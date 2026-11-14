//! The listening queue's commands: `listen.queue.add`, `.list`, `.next`
//! and `.answer` (see [`crate::listen`]). Agents enqueue auditions over
//! the CLI or MCP; stave-play's Listen mode plays them and answers.

use std::path::PathBuf;

use serde_json::json;

use crate::command::{CommandError, CommandInfo, Context, Field, Input, Kind, Outcome};
use crate::listen::{self, Answer, Item, NewItem, Queue};

const QUEUE: Field = Field::flag("queue", Kind::String, "The queue file (default: the user's, see `stave listen queue list`)");

fn path(input: &Input) -> PathBuf {
    input.path("queue").unwrap_or_else(listen::default_path)
}

fn io(path: &std::path::Path) -> impl Fn(std::io::Error) -> CommandError + '_ {
    move |e| CommandError::Failed(format!("{}: {e}", path.display()))
}

fn line(i: &Item) -> String {
    let state = i.answer.map_or("pending".to_string(), |a| format!("answered {}", a.word()));
    format!("{:<5} {:<9} {}  — listen for: {}", i.id, state, i.title, i.listen_for)
}

pub const ADD: CommandInfo = CommandInfo {
    id: "listen.queue.add",
    title: "Queue an audition",
    summary: "Ask a person to listen: a comparison and what to listen for",
    description: "Add an audition to the listening queue: the candidates (the voicing grammar: stage:NAME, \
                  PLUGIN.vst3, au:NAME, wav:FILE, then ;name=value settings and ;label=TEXT; Dry is always \
                  added), the material and what to listen for. stave-play's Listen mode lists it; the \
                  answer (a, b, tie or unclear) and notes come back through `listen queue list`.",
    inputs: &[
        Field::flag("candidate", Kind::Strings, "A candidate to compare (repeat: A, then B, …)").required(),
        Field::flag("listen_for", Kind::String, "What the listener should attend to").required(),
        Field::flag("material", Kind::String, "Program material: a built-in name, role:NAME[#N] or a WAV path"),
        Field::flag("title", Kind::String, "A short name for the list (default: the candidates)"),
        Field::flag("requested_by", Kind::String, "Who is asking (an agent, a PR)"),
        QUEUE,
    ],
    outputs: &[Field::flag("item", Kind::Json, "The queued audition, with its id").required()],
    long_running: false,
};

pub fn add(input: &Input, _cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let candidates = input.strings("candidate");
    if candidates.is_empty() {
        return Err(CommandError::Usage("listen queue add: at least one --candidate".into()));
    }
    let new = NewItem {
        title: input.opt_str("title").map(String::from),
        candidates,
        material: input.opt_str("material").map(String::from),
        listen_for: input.str("listen_for")?.to_string(),
        requested_by: input.opt_str("requested_by").map(String::from),
    };
    let p = path(input);
    let item = listen::update(&p, |q| q.add(new).clone()).map_err(io(&p))?;
    let text = format!("queued {}", line(&item));
    Outcome::ok(json!({ "item": item }), text)
}

pub const LIST: CommandInfo = CommandInfo {
    id: "listen.queue.list",
    title: "List the listening queue",
    summary: "The auditions waiting, and the answers given",
    description: "Every audition in the listening queue, oldest first: pending ones, and answered ones with \
                  their answer and notes. --pending leaves the answered out.",
    inputs: &[Field::flag("pending", Kind::Bool, "Only the auditions still waiting"), QUEUE],
    outputs: &[
        Field::flag("items", Kind::Json, "The auditions").required(),
        Field::flag("path", Kind::String, "The queue file").required(),
    ],
    long_running: false,
};

pub fn list(input: &Input, _cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let p = path(input);
    let q = Queue::load(&p).map_err(io(&p))?;
    let items: Vec<&Item> = q.items.iter().filter(|i| !input.flag("pending") || i.pending()).collect();
    let mut text = format!("{} ({} pending)\n", p.display(), q.pending().count());
    for i in &items {
        text.push_str(&line(i));
        if !i.notes.is_empty() {
            text.push_str(&format!("\n      notes: {}", i.notes));
        }
        text.push('\n');
    }
    Outcome::ok(json!({ "items": items, "path": p }), text.trim_end().to_string())
}

pub const NEXT: CommandInfo = CommandInfo {
    id: "listen.queue.next",
    title: "Next audition",
    summary: "The oldest audition still waiting",
    description: "The oldest pending audition in the listening queue, or none.",
    inputs: &[QUEUE],
    outputs: &[Field::flag("item", Kind::Json, "The audition, or null").required()],
    long_running: false,
};

pub fn next(input: &Input, _cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let p = path(input);
    let q = Queue::load(&p).map_err(io(&p))?;
    let item = q.next();
    let text = item.map_or_else(|| "nothing waiting".to_string(), line);
    Outcome::ok(json!({ "item": item }), text)
}

pub const ANSWER: CommandInfo = CommandInfo {
    id: "listen.queue.answer",
    title: "Answer an audition",
    summary: "Record which sounded better (a, b, tie, unclear) and notes",
    description: "Record the listener's answer to an audition: a or b (the candidate lettered so), tie (equally \
                  good, or no meaningful difference) or unclear (the material doesn't show the difference), \
                  with notes. Answering again replaces the answer.",
    inputs: &[
        Field::positional("id", Kind::String, "The audition, L1, L2, …"),
        Field::positional("answer", Kind::Choice(Answer::WORDS), "a, b, tie or unclear"),
        Field::flag("notes", Kind::String, "What was heard").default(""),
        QUEUE,
    ],
    outputs: &[Field::flag("item", Kind::Json, "The answered audition").required()],
    long_running: false,
};

pub fn answer(input: &Input, _cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let id = input.str("id")?.to_string();
    let a = Answer::parse(input.str("answer")?).ok_or_else(|| CommandError::Usage("answer: a, b, tie or unclear".into()))?;
    let notes = input.opt_str("notes").unwrap_or_default().to_string();
    let p = path(input);
    let item = listen::update(&p, |q| q.answer(&id, a, &notes).cloned())
        .map_err(io(&p))?
        .ok_or_else(|| CommandError::Usage(format!("no audition {id}")))?;
    let text = format!("answered {}", line(&item));
    Outcome::ok(json!({ "item": item }), text)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{registry, Cancel, Context, Progress, Workspace};

    fn run(id: &str, input: serde_json::Value) -> crate::Outcome {
        let mut ws = Workspace::default();
        let mut progress = |_: &Progress| {};
        let mut cx = Context::new(&mut ws, &mut progress, Cancel::default());
        registry().run(id, input, &mut cx).unwrap()
    }

    #[test]
    fn an_agent_queues_and_the_answer_comes_back() {
        let dir = std::env::temp_dir().join(format!("stave-listen-cmd-{}", std::process::id()));
        let q = dir.join("q.json").display().to_string();
        let added = run(
            "listen.queue.add",
            json!({ "candidate": ["stage:clipper/diode", "stage:clipper/tube"], "listen_for": "fizz", "queue": q }),
        );
        let id = added.data["item"]["id"].as_str().unwrap().to_string();
        assert_eq!(run("listen.queue.next", json!({ "queue": q })).data["item"]["id"], id.as_str());
        let answered = run("listen.queue.answer", json!({ "id": id, "answer": "b", "notes": "smoother", "queue": q }));
        assert_eq!(answered.data["item"]["answer"], "b");
        assert!(run("listen.queue.next", json!({ "queue": q })).data["item"].is_null());
        let list = run("listen.queue.list", json!({ "queue": q }));
        assert_eq!(list.data["items"][0]["notes"], "smoother");
        assert!(run("listen.queue.list", json!({ "queue": q, "pending": true })).data["items"].as_array().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
