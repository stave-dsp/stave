//! Voicing: loudness-matched comparisons by ear (DSP.md R21). Pick
//! program material and up to nine candidates, prepare (every candidate
//! runs offline, latency-compensated, and is loudness-matched to 0.1 LU),
//! then listen sighted, in blind ABX trials, or in MUSHRA-style rating
//! trials, or null each candidate against the reference (DSP.md G5:
//! aligned, judged by NMR, the residual played back); every session is
//! logged, and a chosen sound can be blessed.
//!
//! ```text
//! mod.rs       the voicing state: setup, preparing, the live session
//! ui.rs        the Voicing view: setup, transport, keys, Advanced
//! simple.rs    the default screen: Dry / A / B, which sounds better,
//!              the guided blind test
//! modes.rs     the listening modes' panels (and the null test's)
//! null.rs      null tests against the reference: alignment, NMR, residual
//! builtin.rs   built-in program material (synthesised, deterministic)
//! material.rs  material: built-in, WAV, sample library; checksums
//! candidate.rs what is compared, and its command-line form
//! takes.rs     running candidates, latency, the loudness match
//! player.rs    click-free switching playback (an Instance)
//! trials.rs    ABX and rating runs (randomised, seeded)
//! log.rs       the session log and summary
//! bless.rs     "bless this sound", and re-verifying blessed takes
//! rng.rs       a seeded generator
//! ```

mod bless;
mod builtin;
pub(crate) mod candidate;
mod log;
mod material;
mod modes;
pub(crate) mod null;
mod player;
mod rng;
mod simple;
use stave_test::sha256;
mod takes;
#[cfg(test)]
mod tests;
mod trials;
mod ui;
#[cfg(test)]
mod ui_tests;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

use stave_host::config::HostConfig;
use stave_host::param::Param;
use stave_measure::listening::DEFAULT_ABX_TRIALS;

use crate::audio::{self, AudioStream, Shared};

pub use bless::{records as bless_records, verify as verify_blessed};
pub use builtin::BUILTINS;
pub use candidate::Candidate;
pub use log::default_dir;

use log::{History, Meta};
use material::Material;
use player::{Player, Transport};
use takes::Prepared;
use trials::{AbxRun, RatingRun};

/// Candidates a session holds: the digit keys 1–9 select them.
pub const MAX_CANDIDATES: usize = 9;

/// What the command line asks the Voicing view to start with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Launch {
    pub candidates: Vec<String>,
    pub material: Option<String>,
    pub trim_db: Option<f64>,
    pub abx_trials: Option<u32>,
    pub dir: Option<PathBuf>,
}

impl Launch {
    pub fn wants_voicing(&self) -> bool {
        !self.candidates.is_empty()
    }
}

/// Which material the setup holds.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialChoice {
    Builtin(String),
    File(PathBuf),
    /// Item `.1` of a corpus role (DSP.md R22).
    Role(String, usize),
}

impl MaterialChoice {
    /// `--material`'s forms: a built-in name (or `builtin:NAME`),
    /// `role:NAME[#N]`, else a WAV path.
    pub fn parse(m: &str) -> Self {
        let name = m.strip_prefix("builtin:").unwrap_or(m);
        if builtin::find(name).is_some() {
            Self::Builtin(name.into())
        } else if let Some((role, index)) = material::parse_role(m) {
            Self::Role(role, index)
        } else {
            Self::File(candidate::expand_home(m))
        }
    }

    fn load(&self, sample_rate: f64) -> Result<Material, material::MaterialError> {
        match self {
            Self::Builtin(n) => Material::builtin(n, sample_rate),
            Self::File(p) => Material::file(p, sample_rate),
            Self::Role(r, i) => Material::corpus(r, *i, sample_rate),
        }
    }
}

/// One candidate row of the setup.
pub struct Row {
    pub candidate: Candidate,
    /// The params, read once from a probe instance, for the editor.
    pub params: Vec<Param>,
    pub editing: bool,
    /// Whether the label was typed (else it follows the settings).
    pub custom_label: bool,
}

pub struct Setup {
    pub material: MaterialChoice,
    /// The loaded material and the rate it was loaded at.
    loaded: Option<Material>,
    pub rows: Vec<Row>,
    pub reference: Option<usize>,
    pub trim_db: f64,
    pub anchor: bool,
    pub abx_trials: u32,
    pub rating_trials: u32,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            material: MaterialChoice::Builtin(BUILTINS[0].name.into()),
            loaded: None,
            rows: Vec::new(),
            reference: None,
            trim_db: 0.0,
            anchor: true,
            abx_trials: DEFAULT_ABX_TRIALS,
            rating_trials: 1,
        }
    }
}

struct Preparing {
    rx: mpsc::Receiver<Result<Prepared, String>>,
    progress: Arc<AtomicU32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The default screen: switch sources, say which sounds better (the
    /// guided blind test lives here too).
    Listen,
    Abx,
    Rating,
    /// Each candidate nulled against the reference (DSP.md G5).
    Null,
}

/// A prepared session being listened to.
pub struct Live {
    prepared: Prepared,
    transport: Arc<Transport>,
    stream: Option<AudioStream>,
    mode: Mode,
    abx_pair: (usize, usize),
    abx: Option<AbxRun>,
    rating: Option<RatingRun>,
    history: History,
    meta: Meta,
    saved: Option<(PathBuf, String)>,
    abx_trials: u32,
    rating_trials: u32,
    overview: Vec<(f32, f32)>,
    restart_on_switch: bool,
    show_score: bool,
    pref_notes: String,
    /// The rating slot last played (arrow keys move its slider).
    rating_focus: usize,
    /// The unprocessed input's candidate (always present: added when
    /// the setup had none).
    dry: usize,
    /// The other candidates in letter order (A, B, …): the reference
    /// first.
    letters: Vec<usize>,
    /// The guided blind test, while it runs.
    blind: Option<simple::Blind>,
    /// Whether the Advanced disclosure is open.
    advanced_open: bool,
    /// The last "Unclear" answer (an index into the preferences), whose
    /// reason the field below it edits.
    unclear: Option<usize>,
}

pub struct Voicing {
    pub setup: Setup,
    preparing: Option<Preparing>,
    live: Option<Live>,
    pub dir: PathBuf,
    status: Option<(String, bool)>,
    library: Option<Vec<(String, PathBuf)>>,
    library_root: Option<PathBuf>,
    /// Prepare on the first frame (candidates came from the command line).
    autostart: bool,
    /// Whether to open the device (tests drive the view without one).
    start_audio: bool,
    /// The last attempt to open the device failed: wait for a retry
    /// instead of trying every frame.
    audio_failed: bool,
    /// Start playing as soon as the next session is live (the material
    /// changed while playing).
    play_when_live: bool,
}

/// What the Voicing view needs from the app around it.
pub struct Ctx<'a> {
    pub config: HostConfig,
    pub shared: &'a Shared,
    /// The main plugin and its params, when one is loaded.
    pub main_plugin: Option<(PathBuf, Vec<Param>)>,
    /// Drop the main plugin's stream (the comparison takes the device).
    pub suspend_main: &'a mut dyn FnMut(),
}

impl Voicing {
    pub fn new(launch: &Launch) -> Result<Self, String> {
        let mut setup = Setup::default();
        if let Some(m) = &launch.material {
            setup.material = MaterialChoice::parse(m);
        }
        if let Some(t) = launch.trim_db {
            setup.trim_db = t;
        }
        if let Some(n) = launch.abx_trials {
            setup.abx_trials = n.max(1);
        }
        for spec in &launch.candidates {
            let (c, is_ref) = Candidate::parse(spec).map_err(|e| format!("--compare {spec}: {e}"))?;
            if is_ref {
                setup.reference = Some(setup.rows.len());
            }
            let custom_label = spec.contains("label=");
            setup.rows.push(Row {
                candidate: c,
                params: Vec::new(),
                editing: false,
                custom_label,
            });
        }
        if setup.rows.len() > MAX_CANDIDATES {
            return Err(format!("at most {MAX_CANDIDATES} candidates (the digit keys)"));
        }
        let library_root = material::library_root();
        Ok(Self {
            setup,
            preparing: None,
            live: None,
            dir: launch.dir.clone().unwrap_or_else(default_dir),
            status: None,
            library: None,
            library_root,
            autostart: launch.wants_voicing(),
            start_audio: true,
            audio_failed: false,
            play_when_live: false,
        })
    }

    /// Whether candidates are being prepared (or about to be).
    pub fn is_preparing(&self) -> bool {
        self.preparing.is_some() || self.autostart
    }

    /// Whether a comparison holds the audio device.
    pub fn is_playing_device(&self) -> bool {
        self.live.as_ref().is_some_and(|l| l.stream.is_some())
    }

    /// Release the device (leaving the view); the session stays, and
    /// coming back tries the device again.
    pub fn release_device(&mut self) {
        self.audio_failed = false;
        if let Some(l) = &mut self.live {
            l.transport.set_playing(false);
            l.stream = None;
        }
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some((text.into(), error));
    }

    /// Start preparing the setup's candidates on a worker thread.
    fn prepare(&mut self, ctx: &mut Ctx) {
        let mut candidates: Vec<Candidate> = self.setup.rows.iter().map(|r| r.candidate.clone()).collect();
        // Dry, the control, is always there to switch to.
        if !candidates.iter().any(|c| matches!(c.source, candidate::Source::Dry)) {
            candidates.push(Candidate::dry());
        }
        if candidates.len() < 2 {
            return self.set_status("add something to compare with the dry sound", true);
        }
        let rate = ctx.config.sample_rate.hz();
        let material = match self.material(rate) {
            Ok(m) => m.clone(),
            Err(e) => return self.set_status(e, true),
        };
        let jobs = match takes::open_jobs(&candidates, &material, self.setup.trim_db, ctx.config) {
            Ok(j) => j,
            Err(e) => return self.set_status(crate::errors::with_causes(&e), true),
        };
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(AtomicU32::new(0));
        let p = progress.clone();
        let (trim, reference, anchor) = (self.setup.trim_db, self.setup.reference, self.setup.anchor);
        let block = ctx.config.max_block.get();
        let spawned = std::thread::Builder::new()
            .name("voicing-prepare".into())
            .spawn(move || {
                let plan = takes::Plan {
                    material,
                    trim_db: trim,
                    candidates,
                    reference,
                    with_anchor: anchor,
                    block_size: block,
                };
                let r = takes::prepare(plan, jobs, &p)
                    .map(|mut prepared| {
                        prepared.nulls = null::all(&prepared);
                        prepared
                    })
                    .map_err(|e| crate::errors::with_causes(&e));
                let _ = tx.send(r);
            });
        match spawned {
            Ok(_) => {
                self.preparing = Some(Preparing { rx, progress });
                self.set_status("preparing…", false);
            }
            Err(e) => self.set_status(format!("could not start preparing: {e}"), true),
        }
    }

    /// The setup's material at `rate`, loading it if needed.
    fn material(&mut self, rate: f64) -> Result<&Material, String> {
        let stale = self
            .setup
            .loaded
            .as_ref()
            .is_none_or(|m| (m.sample_rate - rate).abs() > 0.5 || !self.choice_matches(m));
        if stale {
            let m = self
                .setup
                .material
                .load(rate)
                .map_err(|e| crate::errors::with_causes(&e))?;
            self.setup.loaded = Some(m);
        }
        Ok(self.setup.loaded.as_ref().expect("loaded above"))
    }

    fn choice_matches(&self, m: &Material) -> bool {
        match (&self.setup.material, &m.source) {
            (MaterialChoice::Builtin(n), material::Source::Builtin { name }) => n == name,
            (MaterialChoice::File(p), material::Source::File { path, .. }) => p == path,
            (MaterialChoice::Role(r, i), material::Source::Corpus { role, index, .. }) => r == role && i == index,
            _ => false,
        }
    }

    /// Collect a finished prepare and go live.
    fn poll_prepare(&mut self, ctx: &mut Ctx) {
        let Some(p) = &self.preparing else { return };
        let result = match p.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("preparing stopped unexpectedly".into()),
        };
        self.preparing = None;
        match result {
            Ok(prepared) => self.go_live(prepared, ctx),
            Err(e) => self.set_status(e, true),
        }
    }

    fn go_live(&mut self, prepared: Prepared, ctx: &mut Ctx) {
        let frames = prepared.material.frames();
        let transport = Transport::new(frames);
        let overview = overview(&prepared.material.channels, 1_200);
        let id = format!("{}-{:04x}", log::compact_stamp(), rng::Rng::seed_from_clock() & 0xffff);
        let n = prepared.candidates.len();
        let (dry, letters) = simple::sources(&prepared);
        let mut live = Live {
            transport,
            stream: None,
            mode: Mode::Listen,
            abx_pair: (0, 1.min(n - 1)),
            abx: None,
            rating: None,
            history: History::default(),
            meta: Meta {
                id,
                started: log::now_iso(),
                crossfade_ms: player::CROSSFADE_SECONDS * 1000.0,
                loop_seconds: [0.0, prepared.material.seconds()],
            },
            saved: None,
            abx_trials: self.setup.abx_trials,
            rating_trials: self.setup.rating_trials,
            overview,
            restart_on_switch: false,
            show_score: true,
            pref_notes: String::new(),
            rating_focus: 0,
            dry,
            letters,
            blind: None,
            advanced_open: false,
            unclear: None,
            prepared,
        };
        // Start on Dry, the control.
        live.transport.select(live.take_of(dry));
        live.transport.set_playing(std::mem::take(&mut self.play_when_live));
        let warnings: usize = live.prepared.takes.iter().map(|t| t.warnings.len()).sum();
        self.set_status(
            if warnings > 0 {
                format!("ready ({warnings} warning(s): see Advanced)")
            } else {
                "ready: space plays".into()
            },
            false,
        );
        live.save(&self.dir, &mut self.status);
        self.live = Some(live);
        self.ensure_stream(ctx);
    }

    /// Start the comparison's stream when there is none.
    fn ensure_stream(&mut self, ctx: &mut Ctx) {
        let Some(l) = &mut self.live else { return };
        if l.stream.is_some() {
            return;
        }
        if (l.prepared.sample_rate - ctx.config.sample_rate.hz()).abs() > 0.5 {
            self.end_session();
            return self.set_status("the device rate changed: prepare again", true);
        }
        if !self.start_audio || self.audio_failed {
            return;
        }
        (ctx.suspend_main)();
        let p = &l.prepared;
        // The takes, then each null's residual (true level, raised):
        // `Live::residual_take` indexes them.
        let mut takes: Vec<_> = p
            .takes
            .iter()
            .enumerate()
            .map(|(i, t)| (t.audio.clone(), p.matching.gain(i)))
            .collect();
        if let Ok(nulls) = &p.nulls {
            takes.extend(null::player_takes(nulls));
        }
        let player = Player::new(takes, l.transport.clone(), p.sample_rate, ctx.config.max_block.get());
        match audio::start(player, ctx.shared.clone()) {
            Ok(s) => l.stream = Some(s),
            Err(e) => {
                self.audio_failed = true;
                let msg = format!("could not start audio: {}", crate::errors::with_causes(&e));
                self.set_status(msg, true);
            }
        }
    }

    /// Listen on other material: end this session (its log is written)
    /// and prepare the same candidates over `choice`.
    fn change_material(&mut self, choice: MaterialChoice) {
        if self.setup.material == choice && self.live.is_some() {
            return;
        }
        let was_playing = self.live.as_ref().is_some_and(|l| l.transport.is_playing());
        self.end_session();
        self.setup.material = choice;
        self.autostart = true;
        self.play_when_live = was_playing;
    }

    /// The material after the current one (built-ins, then any scanned
    /// library files), for "try another clip".
    fn next_material(&self) -> MaterialChoice {
        let mut all: Vec<MaterialChoice> = BUILTINS.iter().map(|b| MaterialChoice::Builtin(b.name.into())).collect();
        if let Some(files) = &self.library {
            all.extend(files.iter().map(|(_, p)| MaterialChoice::File(p.clone())));
        }
        let at = all.iter().position(|m| *m == self.setup.material);
        all[at.map_or(0, |i| (i + 1) % all.len())].clone()
    }

    /// End the live session: write its log, back to the setup.
    fn end_session(&mut self) {
        if let Some(mut l) = self.live.take() {
            l.transport.set_playing(false);
            l.record_runs();
            l.save(&self.dir, &mut self.status);
        }
    }
}

impl Live {
    /// Fold any runs in progress into the history.
    fn record_runs(&mut self) {
        if let Some(r) = &self.abx {
            if !r.trials.is_empty() {
                self.history.record_abx(&self.prepared, r);
            }
        }
        if let Some(r) = &self.rating {
            if !r.trials.is_empty() {
                self.history.record_rating(&self.prepared, r);
            }
        }
    }

    /// Write the log; report where (or why not) in `status`.
    fn save(&mut self, dir: &std::path::Path, status: &mut Option<(String, bool)>) {
        let (start, end) = self.transport.loop_region();
        let rate = self.prepared.sample_rate;
        self.meta.loop_seconds = [start as f64 / rate, end as f64 / rate];
        let built = log::build(&self.prepared, &self.meta, &self.history);
        match log::write(&built, dir) {
            Ok(p) => self.saved = Some((p, built.updated)),
            Err(e) => *status = Some((format!("could not write the session log: {e}"), true)),
        }
    }

    /// Select a take, restarting the loop if asked.
    fn select(&mut self, take: usize) {
        if take != self.transport.selected() && self.restart_on_switch {
            self.transport.seek(self.transport.loop_region().0);
        }
        self.transport.select(take);
    }

    /// The player's take for null `j`'s residual, raised or at its
    /// true level (after the prepared takes: see `ensure_stream`).
    fn residual_take(&self, j: usize, raised: bool) -> usize {
        self.prepared.takes.len() + 2 * j + usize::from(raised)
    }

    fn progress_of_prepare(p: &Preparing) -> f32 {
        p.progress.load(Ordering::Relaxed) as f32 / 1000.0
    }
}

/// Write every built-in item as a 32-bit float WAV into `dir` (to use
/// the same material in a DAW or another tool).
pub fn export_builtins(dir: &std::path::Path, sample_rate: f64) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    BUILTINS
        .iter()
        .map(|b| {
            let path = dir.join(format!("{}.wav", bless::slug(b.name)));
            let audio = stave_host::wav::Audio {
                sample_rate: sample_rate.round() as u32,
                channels: b.make(sample_rate),
            };
            stave_host::wav::write(&path, &audio).map_err(|e| crate::errors::with_causes(&e))?;
            Ok(path)
        })
        .collect()
}

/// Min/max per column of the mono mix: the timeline's waveform.
fn overview(channels: &[Vec<f32>], columns: usize) -> Vec<(f32, f32)> {
    let frames = channels.first().map_or(0, Vec::len);
    if frames == 0 {
        return Vec::new();
    }
    let per = frames.div_ceil(columns).max(1);
    (0..frames)
        .step_by(per)
        .map(|start| {
            let end = (start + per).min(frames);
            (start..end).fold((0.0f32, 0.0f32), |(lo, hi), f| {
                let v = channels.iter().map(|c| c[f]).sum::<f32>() / channels.len() as f32;
                (lo.min(v), hi.max(v))
            })
        })
        .collect()
}
