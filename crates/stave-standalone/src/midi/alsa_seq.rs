//! ALSA sequencer MIDI input: a client with one writable port,
//! subscribed to each chosen readable port, read on a thread of its own
//! (non-blocking, so dropping the input can stop it).

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alsa::seq::{Addr, ClientIter, EvCtrl, EvNote, EventType, PortCap, PortIter, PortSubscribe, PortType, Seq};
use alsa::Direction;

use super::Deliver;

/// Readable MIDI ports other than our own: (address, "client: port").
fn ports(seq: &Seq) -> Vec<(Addr, String)> {
    let me = seq.client_id().unwrap_or(-1);
    let mut out = Vec::new();
    for client in ClientIter::new(seq) {
        if client.get_client() == me {
            continue;
        }
        let client_name = client.get_name().unwrap_or("").to_string();
        for port in PortIter::new(seq, client.get_client()) {
            let caps = port.get_capability();
            if !caps.contains(PortCap::READ | PortCap::SUBS_READ) || !port.get_type().contains(PortType::MIDI_GENERIC) {
                continue;
            }
            let name = port.get_name().unwrap_or("");
            out.push((port.addr(), if name == client_name { name.to_string() } else { format!("{client_name}: {name}") }));
        }
    }
    out
}

pub(super) fn sources() -> Vec<String> {
    match Seq::open(None, Some(Direction::Capture), true) {
        Ok(seq) => ports(&seq).into_iter().map(|(_, n)| n).collect(),
        Err(_) => Vec::new(),
    }
}

pub(super) struct Input {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Input {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn connect(client: &str, wanted: &dyn Fn(&str) -> bool) -> Result<Seq, String> {
    let seq = Seq::open(None, Some(Direction::Capture), true).map_err(|e| format!("ALSA sequencer: {e}"))?;
    let name = CString::new(client.replace('\0', "")).unwrap_or_default();
    let _ = seq.set_client_name(&name);
    let port = seq
        .create_simple_port(c"in", PortCap::WRITE | PortCap::SUBS_WRITE, PortType::MIDI_GENERIC | PortType::APPLICATION)
        .map_err(|e| format!("ALSA sequencer port: {e}"))?;
    let me = Addr { client: seq.client_id().map_err(|e| e.to_string())?, port };
    for (addr, name) in ports(&seq).into_iter().filter(|(_, n)| wanted(n)) {
        let sub = PortSubscribe::empty().map_err(|e| e.to_string())?;
        sub.set_sender(addr);
        sub.set_dest(me);
        if let Err(e) = seq.subscribe_port(&sub) {
            eprintln!("MIDI: could not connect {name}: {e}");
        }
    }
    Ok(seq)
}

pub(super) fn open(client: &str, wanted: &dyn Fn(&str) -> bool, deliver: Deliver) -> Result<Input, String> {
    let seq = connect(client, wanted)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("stave-midi".into())
        .spawn(move || {
            let mut input = seq.input();
            while !flag.load(Ordering::Acquire) {
                let Ok(event) = input.event_input() else {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                };
                let message = match event.get_type() {
                    EventType::Noteon => event.get_data::<EvNote>().map(|n| [0x90 | (n.channel & 0x0F), n.note & 0x7F, n.velocity & 0x7F]),
                    EventType::Noteoff => event.get_data::<EvNote>().map(|n| [0x80 | (n.channel & 0x0F), n.note & 0x7F, n.velocity & 0x7F]),
                    EventType::Controller => event
                        .get_data::<EvCtrl>()
                        .map(|c| [0xB0 | (c.channel & 0x0F), (c.param & 0x7F) as u8, (c.value & 0x7F) as u8]),
                    _ => None,
                };
                if let Some(m) = message {
                    deliver(m);
                }
            }
        })
        .map_err(|e| format!("MIDI thread: {e}"))?;
    Ok(Input { stop, thread: Some(thread) })
}
