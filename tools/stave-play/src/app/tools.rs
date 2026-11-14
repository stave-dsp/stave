//! The shared command registry (`stave_tools`: the CLI's and MCP's
//! commands) as a palette source, and the Output panel that shows what
//! a run returned: its text, its verdict, the files it wrote, its JSON.
//! The same command and the same result whether it ran from here, from
//! `stave …` or from an agent over MCP.
//!
//! A registry command opens its own fresh instance of the loaded
//! plugin, as currently set (its params go along as `set` inputs). It
//! runs on a worker thread when the target is stave's own (a stage, or
//! a plugin with stave telemetry); another vendor's plugin may insist
//! on being created on the main thread, so it runs there, between two
//! frames (the window shows "running" first, then waits).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Map, Value};
use stave_host::param::Param;
use stave_tools::{Cancel, CommandInfo as ToolInfo, Context, Outcome, Progress, Registry, Status, Workspace};

use super::commands::{CommandInfo, Invocation};

/// The prefix that marks a registry command's invocation id.
pub const PREFIX: &str = "tool:";

/// Commands about sessions the palette doesn't keep (each run opens its
/// own instance), and so would mean nothing from here.
const SESSION_COMMANDS: [&str; 3] = ["open", "set", "close"];

/// What the loaded plugin looks like to a registry command.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub spec: String,
    pub rate: f64,
    pub block: usize,
    /// `NAME=VALUE` for every writable param, as currently set.
    pub sets: Vec<String>,
    /// stave's own code: safe to open off the main thread.
    pub ours: bool,
}

impl Target {
    pub fn new(path: &std::path::Path, rate: f64, block: usize, params: &[Param], ours: bool) -> Self {
        let sets = params
            .iter()
            .filter(|p| !p.read_only && !p.flags.hidden && !p.program)
            .map(|p| format!("{}={}", p.name, p.value))
            .collect();
        let spec = path.display().to_string();
        let ours = ours || stave_host::opened::stage_name(path).is_some();
        Self { spec, rate, block, sets, ours }
    }
}

/// The input a command gets from the palette: the target, its rate,
/// block and settings; every other field its default. `None` when the
/// command needs something the palette can't supply (an output path).
pub fn input_for(info: &ToolInfo, target: &Target) -> Option<Value> {
    let mut m = Map::new();
    for f in info.inputs {
        let v = match f.name {
            "target" => Value::from(target.spec.clone()),
            "rate" => Value::from(target.rate),
            "block" => Value::from(target.block as u64),
            "set" => Value::from(target.sets.clone()),
            _ if f.required && f.default.is_none() => return None,
            _ => continue,
        };
        m.insert(f.name.into(), v);
    }
    Some(Value::Object(m))
}

/// A run's result, as the Output panel shows it.
pub struct Finished {
    pub title: String,
    pub id: String,
    pub result: Result<Outcome, String>,
    pub seconds: f64,
}

enum Msg {
    Progress(Progress),
    Done(Result<Outcome, String>),
}

pub struct Running {
    pub title: String,
    pub id: String,
    pub progress: Option<Progress>,
    cancel: Cancel,
    rx: Receiver<Msg>,
    started: Instant,
}

/// A main-thread run waiting for the next frame (so "running" shows).
struct Deferred {
    id: String,
    title: String,
    input: Value,
    /// A frame has been drawn since it was asked for.
    shown: bool,
}

pub struct Tools {
    registry: Arc<Registry>,
    pub running: Option<Running>,
    deferred: Option<Deferred>,
    pub last: Option<Finished>,
}

impl Default for Tools {
    fn default() -> Self {
        Self { registry: Arc::new(stave_tools::registry()), running: None, deferred: None, last: None }
    }
}

impl Tools {
    /// The palette's entries: every registry command the loaded plugin
    /// can be handed to (none without one, except the target-free ones).
    pub fn commands(&self, target: Option<&Target>) -> Vec<CommandInfo> {
        let fallback = Target { spec: String::new(), rate: 48_000.0, block: 256, sets: vec![], ours: true };
        self.registry
            .commands()
            .map(|c| c.info())
            .filter(|i| !SESSION_COMMANDS.contains(&i.id))
            .filter(|i| {
                let needs_target = i.inputs.iter().any(|f| f.name == "target");
                (!needs_target || target.is_some()) && input_for(i, target.unwrap_or(&fallback)).is_some()
            })
            .map(|i| CommandInfo::new("Run", i.title, Invocation::new(&format!("{PREFIX}{}", i.id))))
            .collect()
    }

    pub fn busy(&self) -> bool {
        self.running.is_some() || self.deferred.is_some()
    }

    /// Start `id` against `target`. Refused while another run is going.
    pub fn start(&mut self, id: &str, target: Option<&Target>) -> Result<(), String> {
        if self.busy() {
            return Err("a command is already running (cancel it in the Output panel)".into());
        }
        let info = self.registry.get(id).map(|c| *c.info()).ok_or_else(|| format!("no command {id}"))?;
        let needs_target = info.inputs.iter().any(|f| f.name == "target");
        let fallback = Target { spec: String::new(), rate: 48_000.0, block: 256, sets: vec![], ours: true };
        let t = match target {
            Some(t) => t,
            None if !needs_target => &fallback,
            None => return Err(format!("{}: no plugin loaded", info.title)),
        };
        let input = input_for(&info, t).ok_or_else(|| format!("{}: needs input the palette can't give; use `stave {}`", info.title, info.words().collect::<Vec<_>>().join(" ")))?;
        let (id, title) = (info.id.to_string(), info.title.to_string());
        if needs_target && !t.ours {
            self.deferred = Some(Deferred { id, title, input, shown: false });
            return Ok(());
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Cancel::default();
        let (registry, c, run_id) = (self.registry.clone(), cancel.clone(), id.clone());
        std::thread::Builder::new()
            .name(format!("tool-{id}"))
            .spawn(move || {
                let progress_tx = tx.clone();
                let mut on_progress = |p: &Progress| {
                    let _ = progress_tx.send(Msg::Progress(p.clone()));
                };
                let mut ws = Workspace::default();
                let mut cx = Context::new(&mut ws, &mut on_progress, c);
                let result = registry.run(&run_id, input, &mut cx).map_err(|e| e.to_string());
                let _ = tx.send(Msg::Done(result));
            })
            .map_err(|e| format!("could not start {title}: {e}"))?;
        self.running = Some(Running { title, id, progress: None, cancel, rx, started: Instant::now() });
        Ok(())
    }

    pub fn cancel(&self) {
        if let Some(r) = &self.running {
            r.cancel.cancel();
        }
    }

    /// Collect progress and results (once a frame). A deferred
    /// main-thread run happens here, the frame after it was asked for.
    pub fn poll(&mut self) {
        if let Some(d) = self.deferred.as_mut().filter(|d| !d.shown) {
            d.shown = true;
            return;
        }
        if let Some(d) = self.deferred.take() {
            let started = Instant::now();
            let mut on_progress = |_: &Progress| {};
            let mut ws = Workspace::default();
            let mut cx = Context::new(&mut ws, &mut on_progress, Cancel::default());
            let result = self.registry.run(&d.id, d.input, &mut cx).map_err(|e| e.to_string());
            self.last = Some(Finished { title: d.title, id: d.id, result, seconds: started.elapsed().as_secs_f64() });
            return;
        }
        let Some(r) = &mut self.running else { return };
        loop {
            match r.rx.try_recv() {
                Ok(Msg::Progress(p)) => r.progress = Some(p),
                Ok(Msg::Done(result)) => {
                    let seconds = r.started.elapsed().as_secs_f64();
                    let (title, id) = (r.title.clone(), r.id.clone());
                    self.running = None;
                    self.last = Some(Finished { title, id, result, seconds });
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    let (title, id) = (r.title.clone(), r.id.clone());
                    self.running = None;
                    self.last = Some(Finished { title, id, result: Err("the command's thread stopped".into()), seconds: 0.0 });
                    return;
                }
            }
        }
    }

    /// A one-line state for the status bar.
    pub fn status_line(&self) -> Option<String> {
        if let Some(d) = &self.deferred {
            return Some(format!("{}…", d.title));
        }
        let r = self.running.as_ref()?;
        Some(match &r.progress {
            Some(Progress { done, total: Some(t), .. }) => format!("{} {done:.0}/{t:.0}", r.title),
            _ => format!("{}…", r.title),
        })
    }

    pub fn waiting_title(&self) -> Option<&str> {
        self.deferred.as_ref().map(|d| d.title.as_str())
    }
}

/// The files a run wrote, for the Output panel's links.
pub fn artefacts(f: &Finished) -> Vec<(PathBuf, String)> {
    match &f.result {
        Ok(o) => o.artefacts.iter().map(|a| (a.path.clone(), a.role.clone())).collect(),
        Err(_) => vec![],
    }
}

pub fn passed(f: &Finished) -> Option<bool> {
    f.result.as_ref().ok().map(|o| o.status == Status::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(ours: bool) -> Target {
        Target { spec: "stage:filter/peak".into(), rate: 48_000.0, block: 128, sets: vec!["gain=3".into()], ours }
    }

    #[test]
    fn the_palette_offers_registry_commands_it_can_fill() {
        let tools = Tools::default();
        let ids: Vec<String> = tools.commands(Some(&target(true))).into_iter().map(|c| c.invocation.id).collect();
        for id in ["tool:validate", "tool:bench", "tool:describe", "tool:stages"] {
            assert!(ids.contains(&id.to_string()), "{id} in {ids:?}");
        }
        assert!(!ids.contains(&"tool:render".to_string()), "render needs an output path");
        assert!(!ids.contains(&"tool:open".to_string()), "no sessions from the palette");
        let bare: Vec<String> = tools.commands(None).into_iter().map(|c| c.invocation.id).collect();
        assert!(bare.contains(&"tool:stages".to_string()));
        assert!(
            !bare.iter().any(|id| ["tool:validate", "tool:bench", "tool:describe"].contains(&id.as_str())),
            "with nothing loaded, only target-free commands: {bare:?}"
        );
    }

    #[test]
    fn input_carries_the_target_as_set_and_leaves_defaults_to_the_registry() {
        let r = stave_tools::registry();
        let info = r.get("bench").unwrap().info();
        let v = input_for(info, &target(true)).unwrap();
        assert_eq!(v["target"], "stage:filter/peak");
        assert_eq!(v["block"], 128);
        assert_eq!(v["set"], serde_json::json!(["gain=3"]));
        assert!(v.get("seconds").is_none(), "the registry fills defaults");
    }

    #[test]
    fn a_run_reports_through_the_output() {
        let mut tools = Tools::default();
        tools.start("stages", None).unwrap();
        assert!(tools.start("stages", None).is_err(), "one run at a time");
        let t0 = Instant::now();
        while tools.busy() && t0.elapsed().as_secs() < 30 {
            tools.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let f = tools.last.as_ref().expect("finished");
        assert_eq!(passed(f), Some(true));
        assert!(f.result.as_ref().unwrap().data["stages"].as_array().is_some_and(|a| !a.is_empty()));
    }

    #[test]
    fn another_vendors_plugin_runs_on_the_main_thread_next_frame() {
        let mut tools = Tools::default();
        let mut t = target(false);
        t.spec = "/nowhere/Theirs.vst3".into();
        tools.start("describe", Some(&t)).unwrap();
        assert!(tools.running.is_none() && tools.waiting_title().is_some(), "deferred, not spawned");
        tools.poll();
        assert!(tools.last.is_none(), "a frame shows \"running\" first");
        tools.poll();
        let f = tools.last.as_ref().unwrap();
        assert!(f.result.is_err(), "a missing bundle is an error, reported");
        assert!(!tools.busy());
    }
}
