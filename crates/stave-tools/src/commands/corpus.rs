//! The reference corpus (DSP.md R22) against this machine's sample
//! library: `corpus.list` (each role's picks) and `corpus.check`
//! (required roles left empty, files that drifted from the local lock).
//! Reads the library; never copies, writes or commits any of it.
//! Without a library both say why and pass. (`scripts/corpus.sh lock`
//! records the lock.)

use std::fmt::Write;

use serde_json::json;
use stave_test::corpus::{Corpus, Spec};

use crate::command::{CommandError, CommandInfo, Context, Field, Input, Kind, Outcome};

pub const LIST: CommandInfo = CommandInfo {
    id: "corpus.list",
    title: "List the reference corpus",
    summary: "Each corpus role, what it is for, and its picks in this machine's library",
    description: "The roles crates/stave-test/corpus.spec names (what material each is, which tools use it) \
                  and the items each resolves to in the library STAVE_SAMPLE_LIBRARY names: from the local \
                  lock when it was taken under the role's current rules, else live. A role is what \
                  `--material role:NAME[#N]` and `--program ROLE` take.",
    inputs: &[Field::flag("roles", Kind::Strings, "Only these roles")],
    outputs: &[
        Field::flag("library", Kind::String, "The library root, or why there is none"),
        Field::flag("roles", Kind::Json, "name, about, required, locked, items (name, files)").required(),
    ],
    long_running: false,
};

pub const CHECK: CommandInfo = CommandInfo {
    id: "corpus.check",
    title: "Check the reference corpus",
    summary: "Fail on a required role with no material or a file that drifted from the lock",
    description: "Resolve every role against the library and read each locked item, checking its files \
                  against the local lock (size and time first, the SHA-256 when either moved). Fails \
                  (exit 1) when a required role has no material or a file no longer matches; an absent \
                  library is skipped and passes.",
    inputs: &[],
    outputs: &[
        Field::flag("library", Kind::String, "The library root, or why there is none"),
        Field::flag("skipped", Kind::Bool, "No library: nothing was checked").required(),
        Field::flag("problems", Kind::Strings, "What failed").required(),
        Field::flag("roles", Kind::Json, "name, required, locked, items").required(),
    ],
    long_running: true,
};

pub fn list(input: &Input, _: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let only = input.strings("roles");
    let spec = Spec::committed();
    for r in &only {
        if spec.role(r).is_none() {
            let names: Vec<&str> = spec.roles.iter().map(|r| r.name.as_str()).collect();
            return Err(CommandError::Usage(format!("no role {r} (roles: {})", names.join(", "))));
        }
    }
    let corpus = Corpus::open();
    let mut text = String::new();
    let library = match &corpus {
        Ok(c) => c.root.display().to_string(),
        Err(skip) => skip.to_string(),
    };
    let _ = writeln!(text, "library: {library}");
    let mut roles = Vec::new();
    for role in spec.roles.iter().filter(|r| only.is_empty() || only.contains(&r.name)) {
        let (items, locked) = match &corpus {
            Ok(c) => (c.role(&role.name).unwrap_or_default(), c.locked(role)),
            Err(_) => (Vec::new(), false),
        };
        let _ = writeln!(
            text,
            "role:{:<14} {} item(s){}{}\n    {}",
            role.name,
            items.len(),
            if role.required { ", required" } else { "" },
            if locked { ", locked" } else { "" },
            role.about
        );
        for i in &items {
            let _ = writeln!(text, "    #{} {}", items.iter().position(|x| x == i).unwrap_or(0), i.name);
        }
        let items: Vec<_> = items
            .iter()
            .map(|i| json!({ "name": i.name, "files": i.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>() }))
            .collect();
        roles.push(json!({ "name": role.name, "about": role.about, "required": role.required, "locked": locked, "items": items }));
    }
    Outcome::ok(json!({ "library": library, "roles": roles }), text)
}

pub fn check(_: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let corpus = match Corpus::open() {
        Ok(c) => c,
        Err(skip) => {
            return Outcome::ok(
                json!({ "library": skip.to_string(), "skipped": true, "problems": [], "roles": [] }),
                skip.to_string(),
            )
        }
    };
    let mut problems = Vec::new();
    let mut roles = Vec::new();
    let mut text = format!("library: {}\n", corpus.root.display());
    let total = corpus.spec.roles.len() as f64;
    for (n, role) in corpus.spec.roles.iter().enumerate() {
        cx.checkpoint()?;
        cx.progress(n as f64, Some(total), format!("role {}", role.name));
        let locked = corpus.locked(role);
        let items = corpus.role(&role.name).unwrap_or_default();
        if items.is_empty() && role.required {
            problems.push(format!("role {}: required, no material in the library", role.name));
        }
        if locked {
            for item in &items {
                if let Err(e) = corpus.load(item) {
                    problems.push(format!("role {} item {}: {e}", role.name, item.name));
                }
            }
        }
        let _ = writeln!(
            text,
            "role:{:<14} {} item(s){}{}",
            role.name,
            items.len(),
            if role.required { ", required" } else { "" },
            if locked { ", locked" } else { ", unlocked (scripts/corpus.sh lock)" }
        );
        roles.push(json!({ "name": role.name, "required": role.required, "locked": locked, "items": items.len() }));
    }
    for p in &problems {
        let _ = writeln!(text, "PROBLEM {p}");
    }
    text.push_str(if problems.is_empty() { "corpus: ok" } else { "corpus: problems" });
    let failed = !problems.is_empty();
    Ok(Outcome::ok(
        json!({ "library": corpus.root, "skipped": false, "problems": problems, "roles": roles }),
        text,
    )?
    .failed_if(failed))
}
