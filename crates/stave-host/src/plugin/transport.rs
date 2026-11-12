//! The transport a hosted plugin sees (`ProcessData.processContext`):
//! playing at a steady tempo from time zero. Tempo-synced plugins
//! (LFOs, delays) read it; a null context makes some of them fall back
//! to defaults or misbehave.

use stave_vst3_sys::ProcessContext;

// ProcessContext::state bits (VST3 ivstprocesscontext.h).
const PLAYING: u32 = 1 << 1;
const PROJECT_TIME_MUSIC_VALID: u32 = 1 << 9;
const BAR_POSITION_VALID: u32 = 1 << 11;
const TEMPO_VALID: u32 = 1 << 10;
const TIME_SIG_VALID: u32 = 1 << 13;
const CONT_TIME_VALID: u32 = 1 << 17;

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct TransportConfig {
    pub tempo_bpm: f64,
    pub time_sig: (i32, i32),
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            tempo_bpm: 120.0,
            time_sig: (4, 4),
        }
    }
}

pub(crate) struct Transport {
    config: TransportConfig,
    sample_rate: f64,
    frame: i64,
    ctx: ProcessContext,
}

impl Transport {
    pub fn new(config: TransportConfig, sample_rate: f64) -> Self {
        // SAFETY: ProcessContext is plain-old-data; all-zero is valid.
        let ctx = unsafe { std::mem::zeroed() };
        Self {
            config,
            sample_rate,
            frame: 0,
            ctx,
        }
    }

    /// The context for the block starting now; then advance `frames`.
    pub fn next_block(&mut self, frames: usize) -> *mut ProcessContext {
        let quarters = self.frame as f64 / self.sample_rate * self.config.tempo_bpm / 60.0;
        let (num, den) = self.config.time_sig;
        let bar_len = f64::from(num) * 4.0 / f64::from(den);
        self.ctx = ProcessContext {
            state: PLAYING
                | PROJECT_TIME_MUSIC_VALID
                | BAR_POSITION_VALID
                | TEMPO_VALID
                | TIME_SIG_VALID
                | CONT_TIME_VALID,
            sample_rate: self.sample_rate,
            project_time_samples: self.frame,
            continuous_time_samples: self.frame,
            project_time_music: quarters,
            bar_position_music: (quarters / bar_len).floor() * bar_len,
            tempo: self.config.tempo_bpm,
            time_sig_numerator: num,
            time_sig_denominator: den,
            ..self.ctx
        };
        self.frame += frames as i64;
        &mut self.ctx
    }
}

// Every `unsafe` block in these tests plays the plugin: it calls this
// module's C ABI with pointers the test owns.
#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    #[test]
    fn advances_samples_and_musical_time() {
        let mut t = Transport::new(TransportConfig::default(), 48_000.0);
        let first = unsafe { &*t.next_block(24_000) };
        assert_eq!(
            (first.project_time_samples, first.project_time_music),
            (0, 0.0)
        );
        let second = unsafe { &*t.next_block(24_000) };
        // Half a second at 120 bpm is one quarter note.
        assert_eq!(second.project_time_samples, 24_000);
        assert!((second.project_time_music - 1.0).abs() < 1e-12);
        assert_eq!(second.tempo, 120.0);
        assert!(second.state & PLAYING != 0 && second.state & TEMPO_VALID != 0);
    }

    #[test]
    fn bar_position_is_the_start_of_the_current_bar() {
        let config = TransportConfig {
            tempo_bpm: 60.0,
            time_sig: (3, 4),
        };
        let mut t = Transport::new(config, 1_000.0);
        t.next_block(4_000); // 4 quarters elapse
        let ctx = unsafe { &*t.next_block(1) };
        assert_eq!((ctx.project_time_music, ctx.bar_position_music), (4.0, 3.0));
    }
}
