//! WinMM MIDI input: `midiInOpen` per chosen device with a callback
//! function. WinMM delivers channel messages already parsed (running
//! status resolved), packed into a word.

use core::ffi::c_void;

use super::Deliver;

type Hmidiin = *mut c_void;

const CALLBACK_FUNCTION: u32 = 0x0003_0000;
const MIM_DATA: u32 = 0x3C3;

#[repr(C)]
struct MidiInCapsW {
    mid: u16,
    pid: u16,
    driver_version: u32,
    name: [u16; 32],
    support: u32,
}

#[link(name = "winmm")]
extern "system" {
    fn midiInGetNumDevs() -> u32;
    fn midiInGetDevCapsW(id: usize, caps: *mut MidiInCapsW, size: u32) -> u32;
    fn midiInOpen(handle: *mut Hmidiin, id: u32, callback: usize, instance: usize, flags: u32) -> u32;
    fn midiInStart(handle: Hmidiin) -> u32;
    fn midiInStop(handle: Hmidiin) -> u32;
    fn midiInReset(handle: Hmidiin) -> u32;
    fn midiInClose(handle: Hmidiin) -> u32;
}

fn devices() -> Vec<(u32, String)> {
    // SAFETY: a plain query.
    let count = unsafe { midiInGetNumDevs() };
    (0..count)
        .filter_map(|id| {
            let mut caps = MidiInCapsW { mid: 0, pid: 0, driver_version: 0, name: [0; 32], support: 0 };
            // SAFETY: `caps` is writable and its size is passed.
            let status = unsafe { midiInGetDevCapsW(id as usize, &mut caps, core::mem::size_of::<MidiInCapsW>() as u32) };
            (status == 0).then(|| {
                let len = caps.name.iter().position(|&c| c == 0).unwrap_or(caps.name.len());
                (id, String::from_utf16_lossy(&caps.name[..len]))
            })
        })
        .collect()
}

pub(super) fn sources() -> Vec<String> {
    devices().into_iter().map(|(_, n)| n).collect()
}

pub(super) struct Input {
    handles: Vec<Hmidiin>,
    deliver: *mut Deliver,
}

// SAFETY: WinMM handles may be used from any thread; the deliver box is
// freed only after every handle is closed.
unsafe impl Send for Input {}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: our open handles; once closed no callback runs, so the
        // box they pointed at is ours to free.
        unsafe {
            for &h in &self.handles {
                midiInStop(h);
                midiInReset(h);
                midiInClose(h);
            }
            drop(Box::from_raw(self.deliver));
        }
    }
}

unsafe extern "system" fn callback(_handle: Hmidiin, msg: u32, instance: usize, param1: usize, _param2: usize) {
    if msg != MIM_DATA {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: `instance` is the live Deliver box `open` passed.
        let deliver = unsafe { &*(instance as *const Deliver) };
        let [status, a, b, _] = (param1 as u32).to_le_bytes();
        if (0x80..0xF0).contains(&status) {
            deliver([status, a, b]);
        }
    });
}

pub(super) fn open(_client: &str, wanted: &dyn Fn(&str) -> bool, deliver: Deliver) -> Result<Input, String> {
    let mut input = Input { handles: Vec::new(), deliver: Box::into_raw(Box::new(deliver)) };
    for (id, name) in devices().into_iter().filter(|(_, n)| wanted(n)) {
        let mut handle: Hmidiin = core::ptr::null_mut();
        // SAFETY: `callback` has the MidiInProc signature; the instance
        // pointer outlives the handle (freed in `Input::drop`).
        let status = unsafe { midiInOpen(&mut handle, id, callback as *const () as usize, input.deliver as usize, CALLBACK_FUNCTION) };
        if status != 0 {
            eprintln!("MIDI: could not open {name} ({status})");
            continue;
        }
        // SAFETY: the handle just opened.
        unsafe { midiInStart(handle) };
        input.handles.push(handle);
    }
    Ok(input)
}
