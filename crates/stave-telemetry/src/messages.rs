//! The plugin's recent log messages, readable by the host while the
//! plugin runs. A ring of seqlocked slots: a writer claims a slot by
//! making its sequence odd, writes, and publishes the even sequence of
//! the message's index; a reader keeps what it read only if the sequence
//! was that index's, unchanged across the read.

use alloc::string::String;
use core::sync::atomic::AtomicU32;

use crate::sync::{fence, zeroed, zeroed_array, AtomicU16, AtomicU64, AtomicU8, Ordering};
use crate::{MESSAGE_BYTES, MESSAGE_SLOTS};

const WORDS: usize = MESSAGE_BYTES / 8;

/// One slot of the ring, seqlocked: odd while a writer holds it, then
/// the even sequence of the message it holds.
#[repr(C)]
pub struct Message {
    sequence: AtomicU64,
    severity: AtomicU8,
    len: AtomicU16,
    text: [AtomicU64; WORDS],
}

impl Message {
    zeroed! {
        pub fn zeroed() -> Self {
            Self {
                sequence: AtomicU64::new(0),
                severity: AtomicU8::new(0),
                len: AtomicU16::new(0),
                #[cfg(not(loom))]
                text: [const { AtomicU64::new(0) }; WORDS],
                #[cfg(loom)]
                text: core::array::from_fn(|_| AtomicU64::new(0)),
            }
        }
    }

    /// Write message `index` here; `false` (nothing written) when
    /// another writer holds the slot. `text` past `MESSAGE_BYTES` is cut
    /// at a character boundary.
    pub fn write(&self, index: u64, severity: u8, text: &str) -> bool {
        let current = self.sequence.load(Ordering::Relaxed);
        if current % 2 == 1
            || self
                .sequence
                .compare_exchange(current, current | 1, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
        {
            return false;
        }
        fence(Ordering::Release);
        let mut end = text.len().min(MESSAGE_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let bytes = &text.as_bytes()[..end];
        for (word, chunk) in self.text.iter().zip(bytes.chunks(8)) {
            let mut le = [0u8; 8];
            le[..chunk.len()].copy_from_slice(chunk);
            word.store(u64::from_le_bytes(le), Ordering::Relaxed);
        }
        self.severity.store(severity, Ordering::Relaxed);
        self.len.store(end as u16, Ordering::Relaxed);
        self.sequence.store(published(index), Ordering::Release);
        true
    }

    /// Message `index` as (severity, text), if this slot holds it and no
    /// writer moved it during the read (never a torn message).
    pub fn read(&self, index: u64) -> Option<(u8, String)> {
        let before = self.sequence.load(Ordering::Acquire);
        if before != published(index) {
            return None;
        }
        let severity = self.severity.load(Ordering::Relaxed);
        let len = (self.len.load(Ordering::Relaxed) as usize).min(MESSAGE_BYTES);
        let mut bytes = [0u8; MESSAGE_BYTES];
        for (word, chunk) in self.text.iter().zip(bytes.chunks_mut(8)) {
            chunk.copy_from_slice(&word.load(Ordering::Relaxed).to_le_bytes());
        }
        fence(Ordering::Acquire);
        (self.sequence.load(Ordering::Relaxed) == before)
            .then(|| (severity, String::from_utf8_lossy(&bytes[..len]).into_owned()))
    }
}

#[repr(C)]
pub struct Messages {
    written: AtomicU64,
    /// Lost to a slot another writer held at the same moment.
    dropped: AtomicU32,
    slots: [Message; MESSAGE_SLOTS],
}

impl Messages {
    zeroed! {
        pub fn zeroed() -> Self {
            Self {
                written: AtomicU64::new(0),
                dropped: AtomicU32::new(0),
                slots: zeroed_array![Message; MESSAGE_SLOTS],
            }
        }
    }

    /// Any thread. `text` past `MESSAGE_BYTES` is cut at a character
    /// boundary.
    pub fn push(&self, severity: u8, text: &str) {
        let index = self.written.fetch_add(1, Ordering::Relaxed);
        if !self.slots[index as usize % MESSAGE_SLOTS].write(index, severity, text) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Messages with index ≥ `from` still in the ring, oldest first, as
    /// (index, severity, text). The next call passes the last index + 1.
    pub fn read_from(&self, from: u64) -> impl Iterator<Item = (u64, u8, String)> + '_ {
        let written = self.written.load(Ordering::Acquire);
        let oldest = written.saturating_sub(MESSAGE_SLOTS as u64).max(from);
        (oldest..written).filter_map(move |index| {
            let (severity, text) = self.slots[index as usize % MESSAGE_SLOTS].read(index)?;
            Some((index, severity, text))
        })
    }

    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The even sequence a slot holds once message `index` is in it (0 is
/// "never written").
fn published(index: u64) -> u64 {
    2 * (index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec::Vec;

    fn texts(messages: &Messages, from: u64) -> Vec<(u64, u8, String)> {
        messages.read_from(from).collect()
    }

    #[test]
    fn messages_read_back_in_order_from_an_index() {
        let messages = Messages::zeroed();
        messages.push(3, "first");
        messages.push(5, "second");
        assert_eq!(
            texts(&messages, 0),
            [(0, 3, "first".into()), (1, 5, "second".into())]
        );
        assert_eq!(texts(&messages, 1), [(1, 5, "second".into())]);
        assert!(texts(&messages, 2).is_empty());
    }

    #[test]
    fn the_ring_keeps_the_newest_and_cuts_long_text_at_a_character() {
        let messages = Messages::zeroed();
        for i in 0..MESSAGE_SLOTS + 3 {
            messages.push(0, &format!("m{i}"));
        }
        let kept = texts(&messages, 0);
        assert_eq!(kept.len(), MESSAGE_SLOTS);
        assert_eq!(kept[0].2, "m3");
        let long = "é".repeat(MESSAGE_BYTES);
        messages.push(0, &long);
        let (_, _, text) = messages.read_from(0).last().unwrap();
        assert_eq!(text, "é".repeat(MESSAGE_BYTES / 2));
    }
}
