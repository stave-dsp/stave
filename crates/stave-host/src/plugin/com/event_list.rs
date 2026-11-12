//! Host-side `IEventList` — the note events handed to the plugin each
//! process call. Bounded and allocation-free: capacity is reserved at
//! load, and events past it are counted, not silently lost.

use std::ffi::c_void;
use std::ptr;

use stave_vst3_sys::{
    Event, EventData, IEventList, IEventListVtbl, NoteOffEvent, NoteOnEvent, Tresult, Tuid,
    EVENT_NOTE_OFF, EVENT_NOTE_ON, IID_FUNKNOWN, IID_IEVENT_LIST, K_INVALID_ARGUMENT,
    K_NOT_IMPLEMENTED, K_NO_INTERFACE, K_RESULT_OK,
};

use stave_core::{EventKind, NoteKind};

/// Events one block can carry.
pub(crate) const CAPACITY: usize = 512;

#[repr(C)]
pub(crate) struct EventList {
    vtbl: *const IEventListVtbl,
    events: Vec<Event>,
}

impl EventList {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            vtbl: &VTBL,
            events: Vec::with_capacity(CAPACITY),
        })
    }

    /// Replace the list with `events`; how many did not fit.
    pub fn set(&mut self, events: &[stave_core::Event]) -> usize {
        self.events.clear();
        let fit = events.len().min(CAPACITY);
        self.events.extend(events[..fit].iter().map(to_vst3));
        events.len() - fit
    }

    pub fn as_raw(&mut self) -> *mut IEventList {
        (self as *mut Self).cast()
    }
}

/// A stave event as VST3's (a choke, which VST3 lacks, is an off).
fn to_vst3(e: &stave_core::Event) -> Event {
    let EventKind::Note(n) = e.kind;
    let (r#type, data) = if n.kind == NoteKind::On {
        let note_on = NoteOnEvent {
            channel: n.channel,
            pitch: n.key,
            tuning: 0.0,
            velocity: n.velocity as f32,
            length: 0,
            note_id: n.note_id,
        };
        (EVENT_NOTE_ON, EventData { note_on })
    } else {
        let note_off = NoteOffEvent {
            channel: n.channel,
            pitch: n.key,
            velocity: n.velocity as f32,
            note_id: n.note_id,
            tuning: 0.0,
        };
        (EVENT_NOTE_OFF, EventData { note_off })
    };
    Event {
        bus_index: 0,
        sample_offset: i32::try_from(e.frame).unwrap_or(i32::MAX),
        ppq_position: 0.0,
        flags: 0,
        r#type,
        data,
    }
}

static VTBL: IEventListVtbl = IEventListVtbl {
    query_interface,
    add_ref,
    release,
    get_event_count,
    get_event,
    add_event,
};

/// # Safety
/// `this` is an `EventList` this module handed to `process`, alive for
/// the call.
unsafe fn list<'a>(this: *mut IEventList) -> &'a EventList {
    // SAFETY: this function's contract.
    unsafe { &*(this as *const EventList) }
}

// The callbacks below are called by the plugin with the list this module
// handed out (`this`), under the SDK's contract for their other pointers.

unsafe extern "C" fn query_interface(
    this: *mut IEventList,
    iid: *const Tuid,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: SDK: null or valid.
    let (Some(&iid), Some(obj)) = (unsafe { iid.as_ref() }, unsafe { obj.as_mut() }) else {
        return K_INVALID_ARGUMENT;
    };
    let hit = iid == IID_FUNKNOWN || iid == IID_IEVENT_LIST;
    *obj = if hit { this.cast() } else { ptr::null_mut() };
    if hit {
        K_RESULT_OK
    } else {
        K_NO_INTERFACE
    }
}

unsafe extern "C" fn add_ref(_this: *mut IEventList) -> u32 {
    1 // host-owned for the duration of one process call
}
unsafe extern "C" fn release(_this: *mut IEventList) -> u32 {
    1
}

unsafe extern "C" fn get_event_count(this: *mut IEventList) -> i32 {
    // SAFETY: `this` is ours (see above).
    unsafe { list(this) }.events.len() as i32
}

unsafe extern "C" fn get_event(this: *mut IEventList, index: i32, event: *mut Event) -> Tresult {
    if event.is_null() {
        return K_INVALID_ARGUMENT;
    }
    // SAFETY: `this` is ours (see above).
    let list = unsafe { list(this) };
    match usize::try_from(index).ok().and_then(|i| list.events.get(i)) {
        Some(e) => {
            // SAFETY: SDK: writable. A raw write: the plugin's `Event`
            // may be uninitialized.
            unsafe { event.write(*e) };
            K_RESULT_OK
        }
        None => K_INVALID_ARGUMENT,
    }
}

unsafe extern "C" fn add_event(_this: *mut IEventList, _event: *mut Event) -> Tresult {
    K_NOT_IMPLEMENTED // input list: the plugin only reads
}

// Every `unsafe` block in these tests plays the plugin: it calls this
// module's C ABI with pointers the test owns.
#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    fn read(list: &mut EventList) -> Vec<(u16, i32, i16)> {
        let raw = list.as_raw();
        let vt = unsafe { &**(raw as *const *const IEventListVtbl) };
        (0..unsafe { (vt.get_event_count)(raw) })
            .map(|i| {
                let mut e: Event = unsafe { std::mem::zeroed() };
                assert_eq!(unsafe { (vt.get_event)(raw, i, &mut e) }, K_RESULT_OK);
                (e.r#type, e.sample_offset, unsafe { e.data.note_on.pitch })
            })
            .collect()
    }

    use stave_core::NoteEvent;

    fn on() -> stave_core::Event {
        stave_core::Event::note(0, NoteEvent::on(60, 0.8))
    }

    #[test]
    fn converts_on_and_off_with_offsets() {
        let mut l = EventList::new();
        let off = stave_core::Event::note(100, NoteEvent::off(60, 0.8));
        let dropped = l.set(&[stave_core::Event { frame: 3, ..on() }, off]);
        assert_eq!(dropped, 0);
        assert_eq!(
            read(&mut l),
            vec![(EVENT_NOTE_ON, 3, 60), (EVENT_NOTE_OFF, 100, 60)]
        );
    }

    #[test]
    fn overflow_is_counted() {
        let mut l = EventList::new();
        let notes = vec![on(); CAPACITY + 5];
        assert_eq!(l.set(&notes), 5);
        assert_eq!(read(&mut l).len(), CAPACITY);
    }

    #[test]
    fn bad_indices_are_refused() {
        let mut l = EventList::new();
        l.set(&[on()]);
        let raw = l.as_raw();
        let vt = unsafe { &**(raw as *const *const IEventListVtbl) };
        let mut e: Event = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { (vt.get_event)(raw, 1, &mut e) },
            K_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { (vt.get_event)(raw, -1, &mut e) },
            K_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { (vt.get_event)(raw, 0, ptr::null_mut()) },
            K_INVALID_ARGUMENT
        );
    }
}
