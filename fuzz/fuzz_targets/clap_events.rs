//! The CLAP event path, steered by coverage: Tilt through its CLAP entry,
//! hosted in process, gets blocks whose input events the fuzzer writes —
//! note on/off/choke, parameter values and modulation (known ids and
//! not, any value, NaN included), gestures, transports — at any time
//! offset (unsorted, past the block), in any event space. Every block
//! must answer without an error (a caught panic answers
//! CLAP_PROCESS_ERROR) and every output sample must be finite.
#![no_main]

use core::ffi::c_void;
use core::ptr;

use libfuzzer_sys::fuzz_target;
use stave_clap::sys::*;

/// One event, its header honest about its size and type.
enum Ev {
    Note(ClapEventNote),
    Param(ClapEventParamValue),
    Transport(Box<ClapEventTransport>),
}

impl Ev {
    fn header(&self) -> *const ClapEventHeader {
        match self {
            Ev::Note(e) => &e.header,
            Ev::Param(e) => &e.header,
            Ev::Transport(e) => &e.header,
        }
    }
}

unsafe extern "C" fn size(list: *const ClapInputEvents) -> u32 {
    // SAFETY: `ctx` is the `Vec<Ev>` this target built for the call.
    unsafe { (*(*list).ctx.cast::<Vec<Ev>>()).len() as u32 }
}

unsafe extern "C" fn get(list: *const ClapInputEvents, index: u32) -> *const ClapEventHeader {
    // SAFETY: as in `size`.
    let events = unsafe { &*(*list).ctx.cast::<Vec<Ev>>() };
    events.get(index as usize).map_or(ptr::null(), Ev::header)
}

struct Tape<'a>(&'a [u8]);

impl Tape<'_> {
    fn byte(&mut self) -> u8 {
        let (&b, rest) = self.0.split_first().unwrap_or((&0, &[]));
        self.0 = rest;
        b
    }
    fn word(&mut self) -> u32 {
        u32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }
    fn float(&mut self) -> f64 {
        match self.byte() % 6 {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => -1.0e300,
            _ => f64::from(self.word()) / f64::from(u32::MAX) * 4.0 - 2.0,
        }
    }
}

fn header(t: &mut Tape, size: usize, kind: u16, frames: u32) -> ClapEventHeader {
    ClapEventHeader {
        size: size as u32,
        time: if t.byte() < 230 { t.word() % frames.max(1) } else { t.word() },
        space_id: if t.byte() < 240 { CLAP_CORE_EVENT_SPACE_ID } else { 1 },
        r#type: kind,
        flags: 0,
    }
}

fn event(t: &mut Tape, frames: u32) -> Ev {
    let id = match t.byte() % 5 {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => t.word(),
    };
    match t.byte() % 6 {
        k @ 0..=2 => Ev::Note(ClapEventNote {
            header: header(t, size_of::<ClapEventNote>(), [CLAP_EVENT_NOTE_ON, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_CHOKE][k as usize], frames),
            note_id: t.word() as i32,
            port_index: t.byte() as i16 - 1,
            channel: t.byte() as i16 - 1,
            key: t.byte() as i16 - 1,
            velocity: t.float(),
        }),
        k @ 3..=4 => Ev::Param(ClapEventParamValue {
            header: header(t, size_of::<ClapEventParamValue>(), if k == 3 { CLAP_EVENT_PARAM_VALUE } else { CLAP_EVENT_PARAM_MOD }, frames),
            param_id: id,
            cookie: ptr::null_mut(),
            note_id: -1,
            port_index: -1,
            channel: -1,
            key: -1,
            value: t.float() * 30.0,
        }),
        _ => Ev::Transport(Box::new(ClapEventTransport {
            header: header(t, size_of::<ClapEventTransport>(), CLAP_EVENT_TRANSPORT, frames),
            flags: t.word(),
            song_pos_beats: i64::from(t.word() as i32),
            song_pos_seconds: i64::from(t.word() as i32),
            tempo: t.float() * 200.0,
            tempo_inc: t.float(),
            loop_start_beats: 0,
            loop_end_beats: 0,
            loop_start_seconds: 0,
            loop_end_seconds: 0,
            bar_start: 0,
            bar_number: t.word() as i32,
            tsig_num: u16::from(t.byte()),
            tsig_denom: u16::from(t.byte()),
        })),
    }
}

fuzz_target!(|bytes: &[u8]| {
    let mut t = Tape(bytes);
    // SAFETY (every call below): the plugin's own entry, factory and
    // vtable, called in the order CLAP requires, on live objects.
    unsafe {
        let entry = &stave_tilt_vst3::clap_entry;
        let factory = (entry.get_factory.unwrap())(CLAP_PLUGIN_FACTORY_ID.as_ptr()).cast::<ClapPluginFactory>();
        let plugin = ((*factory).create_plugin.unwrap())(factory, ptr::null(), c"com.ryanbahan.stave.tilt".as_ptr());
        assert!(!plugin.is_null());
        assert!(((*plugin).init.unwrap())(plugin));
        let max = 64u32;
        assert!(((*plugin).activate.unwrap())(plugin, 48_000.0, 1, max));
        assert!(((*plugin).start_processing.unwrap())(plugin));
        let mut memory = vec![0.0f32; 4 * max as usize];
        for _ in 0..(t.byte() % 6 + 1) {
            let frames = u32::from(t.byte()) % (max + 1);
            let n = t.byte() % 24;
            let mut events: Vec<Ev> = (0..n).map(|_| event(&mut t, frames)).collect();
            let list = ClapInputEvents {
                ctx: (&mut events as *mut Vec<Ev>).cast::<c_void>(),
                size: Some(size),
                get: Some(get),
            };
            for (i, s) in memory.iter_mut().enumerate() {
                *s = ((i as f32) * 0.37).sin();
            }
            let base = memory.as_mut_ptr();
            let m = max as usize;
            let mut ins = [base, base.add(m)];
            let mut outs = [base.add(2 * m), base.add(3 * m)];
            let in_buf = ClapAudioBuffer { data32: ins.as_mut_ptr(), data64: ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
            let mut out_buf = ClapAudioBuffer { data32: outs.as_mut_ptr(), data64: ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
            let process = ClapProcess {
                steady_time: -1,
                frames_count: frames,
                transport: ptr::null(),
                audio_inputs: &in_buf,
                audio_outputs: &mut out_buf,
                audio_inputs_count: 1,
                audio_outputs_count: 1,
                in_events: &list,
                out_events: ptr::null(),
            };
            let status = ((*plugin).process.unwrap())(plugin, &process);
            assert_ne!(status, CLAP_PROCESS_ERROR, "a block answered with an error (a caught panic?)");
            let out = &memory[2 * m..2 * m + frames as usize];
            assert!(out.iter().all(|s| s.is_finite()), "non-finite output");
        }
        ((*plugin).stop_processing.unwrap())(plugin);
        ((*plugin).deactivate.unwrap())(plugin);
        ((*plugin).destroy.unwrap())(plugin);
    }
});
