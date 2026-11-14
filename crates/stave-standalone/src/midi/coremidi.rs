//! CoreMIDI input: one client, one input port, a connection per chosen
//! source. CoreMIDI calls the read procedure on its own high-priority
//! thread with a packet list; a [`Parser`] per source turns the bytes
//! into messages.

use core::ffi::{c_char, c_void};
use std::sync::Mutex;

use super::{Deliver, Parser};

type CFStringRef = *const c_void;
type MidiRef = u32;
type ReadProc = unsafe extern "C" fn(list: *const c_void, port_refcon: *mut c_void, conn_refcon: *mut c_void);

const UTF8: u32 = 0x0800_0100;

#[link(name = "CoreMIDI", kind = "framework")]
extern "C" {
    static kMIDIPropertyDisplayName: CFStringRef;
    fn MIDIClientCreate(name: CFStringRef, notify: *const c_void, refcon: *mut c_void, out: *mut MidiRef) -> i32;
    fn MIDIClientDispose(client: MidiRef) -> i32;
    fn MIDIInputPortCreate(client: MidiRef, name: CFStringRef, read: ReadProc, refcon: *mut c_void, out: *mut MidiRef) -> i32;
    fn MIDIGetNumberOfSources() -> usize;
    fn MIDIGetSource(index: usize) -> MidiRef;
    fn MIDIPortConnectSource(port: MidiRef, source: MidiRef, conn_refcon: *mut c_void) -> i32;
    fn MIDIObjectGetStringProperty(object: MidiRef, property: CFStringRef, out: *mut CFStringRef) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(alloc: *const c_void, bytes: *const u8, len: isize, encoding: u32, external: u8) -> CFStringRef;
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFRelease(object: *const c_void);
}

/// An owned CFString (released on drop).
struct CfString(CFStringRef);

impl CfString {
    fn new(s: &str) -> Self {
        // SAFETY: the bytes and length are the str's; the default allocator.
        CfString(unsafe { CFStringCreateWithBytes(core::ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0) })
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: created (owned) by us, released once.
            unsafe { CFRelease(self.0) };
        }
    }
}

/// A source's display name ("Keystep 37", "IAC Driver Bus 1").
fn name_of(source: MidiRef) -> Option<String> {
    let mut s: CFStringRef = core::ptr::null();
    // SAFETY: a MIDI object ref and the framework's property constant;
    // `s` receives a string we own (released by `CfString`).
    let status = unsafe { MIDIObjectGetStringProperty(source, kMIDIPropertyDisplayName, &mut s) };
    if status != 0 || s.is_null() {
        return None;
    }
    let s = CfString(s);
    let mut buf = [0 as c_char; 256];
    // SAFETY: `buf` is writable for its length; the string is live.
    let ok = unsafe { CFStringGetCString(s.0, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
    if ok == 0 {
        return None;
    }
    // SAFETY: CFStringGetCString wrote a nul-terminated string into `buf`.
    Some(unsafe { core::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

fn all_sources() -> Vec<(MidiRef, String)> {
    // SAFETY: plain queries of the MIDI server.
    let count = unsafe { MIDIGetNumberOfSources() };
    (0..count)
        .filter_map(|i| {
            // SAFETY: `i` is below the count just read.
            let source = unsafe { MIDIGetSource(i) };
            (source != 0).then(|| (source, name_of(source).unwrap_or_else(|| format!("MIDI source {}", i + 1))))
        })
        .collect()
}

pub(super) fn sources() -> Vec<String> {
    all_sources().into_iter().map(|(_, name)| name).collect()
}

/// What the read procedure reaches through its refcon.
struct Receiver {
    deliver: Deliver,
    /// One parser per connected source (the connection's refcon is its
    /// index).
    parsers: Mutex<Vec<Parser>>,
}

pub(super) struct Input {
    client: MidiRef,
    receiver: *mut Receiver,
}

// SAFETY: the client ref is a plain id the MIDI server accepts from any
// thread; the receiver is only freed on drop, after the client is gone.
unsafe impl Send for Input {}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: our client; disposing it ends its ports and callbacks,
        // after which the receiver it pointed at is ours to free.
        unsafe {
            MIDIClientDispose(self.client);
            drop(Box::from_raw(self.receiver));
        }
    }
}

/// Offsets in CoreMIDI's packed (4-byte) packet structs: MIDIPacketList
/// { u32 numPackets; MIDIPacket packet[] } and MIDIPacket { u64
/// timeStamp; u16 length; u8 data[] }.
const LIST_PACKETS: usize = 4;
const PACKET_LENGTH: usize = 8;
const PACKET_DATA: usize = 10;

unsafe extern "C" fn read(list: *const c_void, port_refcon: *mut c_void, conn_refcon: *mut c_void) {
    let _ = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        // SAFETY: `port_refcon` is the live Receiver the port was made
        // with (freed only after the client is disposed).
        let receiver = unsafe { &*(port_refcon as *const Receiver) };
        let mut parsers = receiver.parsers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(parser) = parsers.get_mut(conn_refcon as usize) else { return };
        let base = list.cast::<u8>();
        // SAFETY: CoreMIDI hands a valid packet list for the call; the
        // reads follow its layout (MIDIServices.h, MIDIPacketNext).
        unsafe {
            let count = base.cast::<u32>().read_unaligned();
            let mut packet = base.add(LIST_PACKETS);
            for _ in 0..count {
                let length = usize::from(packet.add(PACKET_LENGTH).cast::<u16>().read_unaligned());
                let data = core::slice::from_raw_parts(packet.add(PACKET_DATA), length);
                for &byte in data {
                    if let Some(message) = parser.feed(byte) {
                        (receiver.deliver)(message);
                    }
                }
                let next = packet.add(PACKET_DATA + length);
                packet = if cfg!(target_arch = "aarch64") { next.add(next.align_offset(4)) } else { next };
            }
        }
    }));
}

pub(super) fn open(client_name: &str, wanted: &dyn Fn(&str) -> bool, deliver: Deliver) -> Result<Input, String> {
    let chosen: Vec<MidiRef> = all_sources().into_iter().filter(|(_, n)| wanted(n)).map(|(s, _)| s).collect();
    let receiver = Box::into_raw(Box::new(Receiver { deliver, parsers: Mutex::new(vec![Parser::default(); chosen.len()]) }));
    let mut client = 0;
    let name = CfString::new(client_name);
    // SAFETY: a CFString name, no notification procedure.
    let status = unsafe { MIDIClientCreate(name.0, core::ptr::null(), core::ptr::null_mut(), &mut client) };
    if status != 0 {
        // SAFETY: never shared: no client exists to call back.
        drop(unsafe { Box::from_raw(receiver) });
        return Err(format!("MIDIClientCreate failed ({status})"));
    }
    let input = Input { client, receiver };
    let mut port = 0;
    let port_name = CfString::new("in");
    // SAFETY: our client; `read` has the MIDIReadProc signature and the
    // refcon outlives the client (freed in `Input::drop`).
    let status = unsafe { MIDIInputPortCreate(client, port_name.0, read, receiver.cast(), &mut port) };
    if status != 0 {
        return Err(format!("MIDIInputPortCreate failed ({status})"));
    }
    for (i, &source) in chosen.iter().enumerate() {
        // SAFETY: our port and a source ref the server listed; the
        // connection refcon is a plain index.
        let status = unsafe { MIDIPortConnectSource(port, source, i as *mut c_void) };
        if status != 0 {
            eprintln!("MIDI: could not connect source {} ({status})", i + 1);
        }
    }
    Ok(input)
}
