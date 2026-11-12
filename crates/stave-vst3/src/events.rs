//! Host-event translation: VST3 parameter-change queues and event
//! lists become the table's block-start values and the plugin's
//! typed `Event` stream.

use stave_core::event::NoteKind;
use stave_core::params::Params;
use stave_core::{Event as StaveEvent, EventKind, NoteEvent, ParamChange, ParamChangeKind};

use crate::host::{EventList, ParamChanges};
use crate::sys::*;

/// Every input param queue into `dst`, the block's timeline that
/// processor-owned controls run sample-accurately; the LAST point
/// becomes the table's target. Normalized on the wire (VST3), plain in
/// the store. Returns (written into `dst`, dropped for want of room).
pub(crate) fn drain_param_changes(
    changes: &ParamChanges<'_>,
    table: &dyn Params,
    dst: &mut [ParamChange],
) -> (usize, u32) {
    let mut written = 0;
    let mut dropped = 0u32;
    for queue in changes.queues() {
        let id = queue.id();
        let Some(p) = table.by_id(stave_core::ParamId(id)) else {
            continue;
        };
        let mut last_plain = None;
        for (offset, value) in queue.points() {
            let offset = offset.max(0) as usize;
            let plain = p.denormalize(value);
            if let Some(slot) = dst.get_mut(written) {
                *slot = ParamChange {
                    id: stave_core::ParamId(id),
                    frame: offset,
                    value: plain,
                    kind: ParamChangeKind::Value,
                };
                written += 1;
            } else {
                dropped = dropped.saturating_add(1);
            }
            last_plain = Some(plain);
        }
        if let Some(plain) = last_plain {
            p.set_value(plain);
        }
    }
    (written, dropped)
}

/// Note events into `dst`; returns (written, dropped for want of room).
pub(crate) fn translate_input_events(
    events: &EventList<'_>,
    dst: &mut [StaveEvent],
) -> (usize, u32) {
    let mut written = 0;
    let mut dropped = 0u32;
    for ev in events.iter() {
        let kind = match ev.r#type {
            EVENT_NOTE_ON => {
                // SAFETY: the type tag says the union holds a note-on.
                let n = unsafe { ev.data.note_on };
                EventKind::Note(NoteEvent {
                    kind: NoteKind::On,
                    key: n.pitch,
                    channel: n.channel,
                    velocity: n.velocity as f64,
                    note_id: n.note_id,
                })
            }
            EVENT_NOTE_OFF => {
                // SAFETY: the type tag says the union holds a note-off.
                let n = unsafe { ev.data.note_off };
                EventKind::Note(NoteEvent {
                    kind: NoteKind::Off,
                    key: n.pitch,
                    channel: n.channel,
                    velocity: n.velocity as f64,
                    note_id: n.note_id,
                })
            }
            _ => continue,
        };
        let Some(slot) = dst.get_mut(written) else {
            dropped = dropped.saturating_add(1);
            continue;
        };
        *slot = StaveEvent {
            frame: ev.sample_offset.max(0) as usize,
            kind,
        };
        written += 1;
    }
    (written, dropped)
}
