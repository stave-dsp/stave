//! MIDI input, our own code on each platform's system API: CoreMIDI
//! (macOS), WinMM (Windows), the ALSA sequencer (Linux; PipeWire and
//! JACK MIDI appear there through their bridges). Each backend lists its
//! sources and, while its [`Input`] lives, delivers raw channel messages
//! from the chosen ones to a callback on its own thread.

#[cfg(all(unix, not(target_os = "macos")))]
mod alsa_seq;
#[cfg(target_os = "macos")]
mod coremidi;
#[cfg(windows)]
mod winmm;

#[cfg(all(unix, not(target_os = "macos")))]
use alsa_seq as backend;
#[cfg(target_os = "macos")]
use coremidi as backend;
#[cfg(windows)]
use winmm as backend;

use crate::settings::Midi;

/// Where a message goes: called on the backend's thread.
pub(crate) type Deliver = Box<dyn Fn([u8; 3]) + Send + Sync>;

/// The MIDI sources on this machine, by name.
pub(crate) fn sources() -> Vec<String> {
    backend::sources()
}

/// Open MIDI input: every source the choice names (`Midi::None`: no
/// input, `Ok(None)`). A named source that is gone opens nothing.
pub(crate) fn open(choice: &Midi, client: &str, deliver: Deliver) -> Result<Option<Input>, String> {
    let wanted = |name: &str| match choice {
        Midi::None => false,
        Midi::All => true,
        Midi::Named(n) => n == name,
    };
    if matches!(choice, Midi::None) {
        return Ok(None);
    }
    backend::open(client, &wanted, deliver).map(|inner| Some(Input { _inner: inner }))
}

/// Open MIDI input; dropping it closes every connection.
pub(crate) struct Input {
    _inner: backend::Input,
}

/// Raw MIDI bytes into channel messages (CoreMIDI hands bytes; WinMM
/// and the ALSA sequencer hand parsed messages): running status, real-time
/// bytes between a message's bytes, and system exclusive (skipped).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Default, Clone, Copy)]
pub(crate) struct Parser {
    status: u8,
    data: [u8; 2],
    have: usize,
    sysex: bool,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Parser {
    /// Feed one byte; a complete channel message comes back padded to
    /// three bytes.
    pub(crate) fn feed(&mut self, byte: u8) -> Option<[u8; 3]> {
        match byte {
            0xF8..=0xFF => None, // real-time: does not disturb anything
            0xF0 => {
                self.sysex = true;
                self.status = 0;
                None
            }
            0xF7 => {
                self.sysex = false;
                None
            }
            0xF1..=0xF6 => {
                // System common: cancels running status; not delivered.
                self.status = 0;
                self.sysex = false;
                None
            }
            0x80..=0xEF => {
                self.status = byte;
                self.have = 0;
                self.sysex = false;
                None
            }
            _ if self.sysex || self.status == 0 => None,
            _ => {
                self.data[self.have] = byte;
                self.have += 1;
                let needed = if matches!(self.status & 0xF0, 0xC0 | 0xD0) { 1 } else { 2 };
                if self.have < needed {
                    return None;
                }
                self.have = 0;
                Some([self.status, self.data[0], if needed == 2 { self.data[1] } else { 0 }])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Vec<[u8; 3]> {
        let mut p = Parser::default();
        bytes.iter().filter_map(|&b| p.feed(b)).collect()
    }

    #[test]
    fn running_status_and_real_time_bytes_parse() {
        assert_eq!(parse(&[0x90, 60, 100, 62, 0xF8, 90, 0x80, 60, 0]), [[0x90, 60, 100], [0x90, 62, 90], [0x80, 60, 0]]);
    }

    #[test]
    fn sysex_and_stray_data_are_skipped() {
        assert_eq!(parse(&[1, 2, 0xF0, 0x7E, 0x01, 0xF7, 5, 0xC1, 7]), [[0xC1, 7, 0]]);
    }
}
