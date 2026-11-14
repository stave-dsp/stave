//! The listening queue: auditions waiting for a person's ears. Each one
//! is a ready-to-run comparison (candidates in the voicing grammar, the
//! material, what to listen for). An agent enqueues it with
//! `stave listen queue add`; stave-play's Listen mode lists it, loads
//! it into the Dry / A / B screen, and records the answer (A, B, tie,
//! unclear) and notes here.
//!
//! The queue is a user-local JSON file of specs, never audio:
//! `$STAVE_LISTEN_QUEUE`, else `$XDG_CONFIG_HOME/stave/listen-queue.json`,
//! else `~/.config/stave/listen-queue.json`. Every change is written to
//! a temporary file and renamed over it, so a reader never sees half.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Where the queue lives (see the module docs).
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("STAVE_LISTEN_QUEUE") {
        return PathBuf::from(p);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    config.join("stave").join("listen-queue.json")
}

/// The listener's verdict.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    A,
    B,
    /// Equally good, or no meaningful difference.
    Tie,
    /// Not enough to make a call (another clip or level is needed).
    Unclear,
}

impl Answer {
    pub const WORDS: &'static [&'static str] = &["a", "b", "tie", "unclear"];

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "a" => Some(Self::A),
            "b" => Some(Self::B),
            "tie" => Some(Self::Tie),
            "unclear" => Some(Self::Unclear),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        Self::WORDS[self as usize]
    }
}

/// One audition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// `L1`, `L2`, …: never reused.
    pub id: String,
    /// A short name for the list.
    pub title: String,
    /// What to compare, in the voicing grammar (`stage:…;drive=0.6`,
    /// `wav:FILE`, `PLUGIN.vst3;label=…`). Dry is always added.
    pub candidates: Vec<String>,
    /// The program material (a built-in name, `role:NAME[#N]` or a WAV
    /// path); none: the default.
    #[serde(default)]
    pub material: Option<String>,
    /// What the listener should attend to.
    pub listen_for: String,
    /// Who asked (an agent's name, a PR).
    #[serde(default)]
    pub requested_by: Option<String>,
    /// Unix seconds.
    pub added: u64,
    #[serde(default)]
    pub answer: Option<Answer>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub answered: Option<u64>,
}

impl Item {
    pub fn pending(&self) -> bool {
        self.answer.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub items: Vec<Item>,
}

/// What `add` takes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewItem {
    pub title: Option<String>,
    pub candidates: Vec<String>,
    pub material: Option<String>,
    pub listen_for: String,
    pub requested_by: Option<String>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Queue {
    /// The queue at `path`; an absent file is an empty queue.
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// Append an audition; its id.
    pub fn add(&mut self, new: NewItem) -> &Item {
        self.next_id += 1;
        let id = format!("L{}", self.next_id);
        let title = new.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
            new.candidates.iter().map(|c| c.split(';').next().unwrap_or(c)).collect::<Vec<_>>().join(" vs ")
        });
        self.items.push(Item {
            id,
            title,
            candidates: new.candidates,
            material: new.material,
            listen_for: new.listen_for,
            requested_by: new.requested_by,
            added: now(),
            answer: None,
            notes: String::new(),
            answered: None,
        });
        self.items.last().expect("just pushed")
    }

    pub fn pending(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| i.pending())
    }

    /// The oldest audition still waiting.
    pub fn next(&self) -> Option<&Item> {
        self.pending().next()
    }

    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id.eq_ignore_ascii_case(id))
    }

    /// Record the verdict on `id` (answering again replaces it).
    pub fn answer(&mut self, id: &str, answer: Answer, notes: &str) -> Option<&Item> {
        let item = self.items.iter_mut().find(|i| i.id.eq_ignore_ascii_case(id))?;
        item.answer = Some(answer);
        item.notes = notes.to_string();
        item.answered = Some(now());
        Some(item)
    }
}

/// Load, change, save: one read-modify-write of the file at `path`.
pub fn update<T>(path: &Path, change: impl FnOnce(&mut Queue) -> T) -> io::Result<T> {
    let mut q = Queue::load(path)?;
    let out = change(&mut q);
    q.save(path)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("stave-listen-{tag}-{}", std::process::id())).join("q.json")
    }

    #[test]
    fn add_next_answer_round_trip() {
        let path = tmp("rt");
        let _ = std::fs::remove_file(&path);
        assert!(Queue::load(&path).unwrap().items.is_empty(), "no file: empty");
        let id = update(&path, |q| {
            q.add(NewItem {
                candidates: vec!["stage:clipper/diode;drive=0.6".into(), "stage:clipper/tube".into()],
                listen_for: "fizz on the decay".into(),
                ..NewItem::default()
            })
            .id
            .clone()
        })
        .unwrap();
        assert_eq!(id, "L1");
        let q = Queue::load(&path).unwrap();
        let next = q.next().unwrap();
        assert_eq!(next.title, "stage:clipper/diode vs stage:clipper/tube");
        update(&path, |q| q.answer("l1", Answer::Tie, "same to me").is_some()).unwrap();
        let q = Queue::load(&path).unwrap();
        assert!(q.next().is_none());
        assert_eq!((q.items[0].answer, q.items[0].notes.as_str()), (Some(Answer::Tie), "same to me"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ids_are_never_reused_and_answers_spell_as_words() {
        let mut q = Queue::default();
        q.add(NewItem::default());
        q.items.clear();
        assert_eq!(q.add(NewItem::default()).id, "L2");
        for w in Answer::WORDS {
            assert_eq!(Answer::parse(w).unwrap().word(), *w);
        }
    }
}
