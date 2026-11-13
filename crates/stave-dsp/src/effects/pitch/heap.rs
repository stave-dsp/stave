//! A fixed-capacity max-heap of `(magnitude, coefficient)` pairs: the
//! self-sorting heap phase-gradient integration propagates from
//! (Williams, *Algorithm 232: Heapsort*, CACM 1964). Plain binary heap on
//! a vector reserved once; pushing within the capacity never allocates.

#[derive(Clone, Debug, Default)]
pub(super) struct Heap {
    items: Vec<(f32, u32)>,
}

impl Heap {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    pub(super) fn clear(&mut self) {
        self.items.clear();
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Pushes `(key, value)`.
    ///
    /// Realtime within the capacity; `debug_assert!`s it is not exceeded.
    pub(super) fn push(&mut self, key: f32, value: u32) {
        debug_assert!(self.items.len() < self.items.capacity(), "Heap over capacity");
        self.items.push((key, value));
        let mut i = self.items.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 2;
            if self.items[parent].0 >= self.items[i].0 {
                break;
            }
            self.items.swap(parent, i);
            i = parent;
        }
    }

    /// The value with the largest key, removed.
    pub(super) fn pop(&mut self) -> Option<u32> {
        let last = self.items.len().checked_sub(1)?;
        self.items.swap(0, last);
        let (_, top) = self.items.pop()?;
        let n = self.items.len();
        let mut i = 0;
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut big = i;
            if l < n && self.items[l].0 > self.items[big].0 {
                big = l;
            }
            if r < n && self.items[r].0 > self.items[big].0 {
                big = r;
            }
            if big == i {
                break;
            }
            self.items.swap(i, big);
            i = big;
        }
        Some(top)
    }
}

#[cfg(test)]
mod tests {
    use super::Heap;

    #[test]
    fn pops_in_descending_key_order() {
        let keys = [0.3f32, 2.0, -1.0, 7.5, 0.0, 7.5, 1.25, 3.0];
        let mut heap = Heap::with_capacity(keys.len());
        for (i, &k) in keys.iter().enumerate() {
            heap.push(k, i as u32);
        }
        let mut last = f32::INFINITY;
        let mut seen = 0;
        while let Some(i) = heap.pop() {
            let k = keys[i as usize];
            assert!(k <= last);
            last = k;
            seen += 1;
        }
        assert_eq!(seen, keys.len());
        assert!(heap.is_empty());
    }
}
