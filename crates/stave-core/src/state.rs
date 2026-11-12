//! State chunks: the bytes a host stores in
//! a session or preset and hands back on reload.
//!
//! Design rules, in priority order:
//! 1. **Never lose what you don't understand.** Unknown sections and
//!    unknown param ids (from a NEWER plugin version) are retained on
//!    load and re-emitted on save — a v9 session opened and re-saved
//!    in v8 loses nothing.
//! 2. **Plain values, keyed by stable [`ParamId`]s.** Normalized never
//!    touches the wire: a later range widening must not silently
//!    re-pitch every old session, and the taper layer's plain-grid
//!    quantization (see `params::taper`) never applies twice.
//! 3. **Malformed input is a clean error, never a panic or an
//!    unbounded allocation.** Hosts feed plugins garbage (corrupt
//!    sessions, wrong-plugin chunks); the mutational test at the
//!    bottom of this file holds that line.
//!
//! Wire layout (all little-endian):
//! ```text
//! magic "STVE" | u32 format_version | u32 plugin_version
//! then sections until end-of-bytes:
//!   u32 tag | u32 byte_len | payload[byte_len]
//! tag 1 = PARAMS: u32 count, then count × (u32 id, u64 f64-bits plain)
//! tag 2 = EXTRA:  opaque plugin blob
//! any other tag: skipped by byte_len, PRESERVED for re-save
//! ```

use std::vec::Vec;

use crate::params::ParamId;
use crate::params::Params;

pub const MAGIC: [u8; 4] = *b"STVE";
pub const FORMAT_VERSION: u32 = 1;

const TAG_PARAMS: u32 = 1;
const TAG_EXTRA: u32 = 2;
/// End sentinel: hosts embed our chunk inside larger container streams
/// (VST3 preset files put more data after it); the loader stops here
/// and ignores the rest of the stream. Absent sentinel = legacy chunk,
/// read to end-of-bytes (the v1 fixture corpus predates it).
const TAG_END: u32 = 0;

/// Cap on any single declared length before allocation. Real chunks
/// are kilobytes; a corrupt length must not become a 4 GiB alloc.
const MAX_SECTION_BYTES: usize = 16 * 1024 * 1024;

/// Why a chunk failed to load. Loud, specific, and non-fatal — the
/// caller keeps current state.
#[derive(Debug, PartialEq, Eq)]
pub enum StateError {
    TooShort,
    BadMagic,
    /// Format newer than this binary understands.
    UnsupportedFormat {
        found: u32,
    },
    /// A declared length runs past the end of the chunk (corruption).
    Truncated,
    /// A declared length exceeds the sanity cap (corruption).
    Oversized {
        bytes: usize,
    },
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooShort => write!(f, "chunk shorter than the fixed header"),
            Self::BadMagic => write!(f, "not a STVE chunk"),
            Self::UnsupportedFormat { found } => {
                write!(
                    f,
                    "format v{found} is newer than this binary (max {FORMAT_VERSION})"
                )
            }
            Self::Truncated => write!(f, "declared section length runs past end of chunk"),
            Self::Oversized { bytes } => write!(f, "section of {bytes} bytes exceeds sanity cap"),
        }
    }
}

/// What a successful load found — callers decide what to do with the
/// leftovers (usually: nothing; the store keeps them for re-save).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Params applied to the table.
    pub applied: usize,
    /// Param ids in the chunk with no owner in the table (retained).
    pub unknown_params: usize,
    /// Sections with unrecognized tags (retained).
    pub unknown_sections: usize,
    /// Plugin version the chunk was written by.
    pub plugin_version: u32,
}

/// Per-instance retention of chunk content this binary doesn't
/// understand. Owned by the wrapper; survives load→save cycles.
#[derive(Debug, Default)]
pub struct StateStore {
    unknown_params: Vec<(u32, u64)>,
    unknown_sections: Vec<(u32, Vec<u8>)>,
}

/// Param-level version migration, declared by the plugin author.
/// Applied at load, in order: renames first, then rescales.
/// A plain-value rescale applied when loading older state.
pub type Rescale = fn(f64) -> f64;

#[derive(Default)]
pub struct Migrations {
    renames: Vec<(u32, u32)>,
    rescales: Vec<(u32, Rescale)>,
    versioned_rescales: Vec<(u32, u32, Rescale)>,
}

impl Migrations {
    /// `old` ids found in a chunk load as `new`. Chain across versions
    /// by declaring each hop.
    ///
    /// ```
    /// use stave_core::{state::Migrations, ParamId};
    ///
    /// let m = Migrations::default().renamed(ParamId(4), ParamId(9));
    /// # let _ = m;
    /// ```
    pub fn renamed(mut self, old: ParamId, new: ParamId) -> Self {
        self.renames.push((old.0, new.0));
        self
    }

    /// Rewrite the stored value of `id` on load (unit changes, range
    /// re-anchoring). Runs after renames, keyed by the NEW id.
    pub fn rescaled(mut self, id: ParamId, f: Rescale) -> Self {
        self.rescales.push((id.0, f));
        self
    }

    /// Rewrite `id` only when loading state written before
    /// `first_version_with_current_semantics`.
    ///
    /// Use this when a stable parameter id keeps its host automation identity
    /// but its stored plain unit changes. Newly saved state must not be
    /// transformed again on its next load. Multiple matching version steps
    /// run in declaration order so state can cross more than one semantic
    /// revision.
    pub fn rescaled_before(
        mut self,
        first_version_with_current_semantics: u32,
        id: ParamId,
        transform: Rescale,
    ) -> Self {
        self.versioned_rescales
            .push((first_version_with_current_semantics, id.0, transform));
        self
    }

    fn apply(&self, source_plugin_version: u32, id: u32, value: f64) -> (u32, f64) {
        let id = self
            .renames
            .iter()
            .find(|(old, _)| *old == id)
            .map_or(id, |&(_, new)| new);
        let mut value = self
            .rescales
            .iter()
            .find(|(rid, _)| *rid == id)
            .map_or(value, |&(_, f)| f(value));
        for (_, _, transform) in
            self.versioned_rescales
                .iter()
                .filter(|(first_current_version, rescaled_id, _)| {
                    source_plugin_version < *first_current_version && *rescaled_id == id
                })
        {
            value = transform(value);
        }
        (id, value)
    }
}

/// Serialize the current state: every table param (plain, by id), the
/// plugin's extra blob, and everything retained from newer versions.
pub fn save(table: &dyn Params, plugin_version: u32, extra: &[u8], store: &StateStore) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + table.count() * 12 + extra.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&plugin_version.to_le_bytes());

    // PARAMS section: live table values, then retained unknowns.
    let count = table.count() + store.unknown_params.len();
    let payload_len = 4 + count * 12;
    out.extend_from_slice(&TAG_PARAMS.to_le_bytes());
    out.extend_from_slice(&(payload_len as u32).to_le_bytes());
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for i in 0..table.count() {
        // count() and by_index agree per Params::validate; a hole here
        // would be an author bug caught at instance creation.
        let Some(p) = table.by_index(i) else { continue };
        out.extend_from_slice(&p.meta().id.0.to_le_bytes());
        out.extend_from_slice(&p.value().to_bits().to_le_bytes());
    }
    for &(id, bits) in &store.unknown_params {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
    }

    if !extra.is_empty() {
        out.extend_from_slice(&TAG_EXTRA.to_le_bytes());
        out.extend_from_slice(&(extra.len() as u32).to_le_bytes());
        out.extend_from_slice(extra);
    }

    for (tag, payload) in &store.unknown_sections {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
    }
    out.extend_from_slice(&TAG_END.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// Little-endian reads over a chunk; running out is `Truncated`.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], StateError> {
        let (head, rest) = self
            .bytes
            .split_at_checked(n)
            .ok_or(StateError::Truncated)?;
        self.bytes = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StateError> {
        let (head, rest) = self
            .bytes
            .split_first_chunk::<N>()
            .ok_or(StateError::Truncated)?;
        self.bytes = rest;
        Ok(*head)
    }

    fn u32(&mut self) -> Result<u32, StateError> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Result<u64, StateError> {
        self.array().map(u64::from_le_bytes)
    }
}

/// Parse a chunk and apply it: params into the table (with
/// `migrations`), unknowns into `store`, the EXTRA payload returned
/// for the plugin. On error the table is untouched — parsing completes
/// BEFORE any application, so a corrupt chunk can't half-apply.
pub fn load(
    bytes: &[u8],
    table: &dyn Params,
    migrations: &Migrations,
    store: &mut StateStore,
) -> Result<(LoadReport, Vec<u8>), StateError> {
    // ---- Parse phase (no side effects) ----
    if bytes.len() < 12 {
        return Err(StateError::TooShort);
    }
    let mut chunk = Reader { bytes };
    if chunk.array::<4>()? != MAGIC {
        return Err(StateError::BadMagic);
    }
    let format = chunk.u32()?;
    if format > FORMAT_VERSION {
        // Forward-incompatible FORMAT is refused; forward-compatible
        // content (unknown sections/params) is the normal path below.
        return Err(StateError::UnsupportedFormat { found: format });
    }
    let plugin_version = chunk.u32()?;

    let mut params: Vec<(u32, u64)> = Vec::new();
    let mut extra: Vec<u8> = Vec::new();
    let mut unknown_sections: Vec<(u32, Vec<u8>)> = Vec::new();
    while !chunk.bytes.is_empty() {
        let tag = chunk.u32()?;
        let len = chunk.u32()? as usize;
        if tag == TAG_END {
            // Everything past the sentinel belongs to the host's
            // container, not to us.
            break;
        }
        if len > MAX_SECTION_BYTES {
            return Err(StateError::Oversized { bytes: len });
        }
        let payload = chunk.take(len)?;
        match tag {
            TAG_PARAMS => {
                let mut section = Reader { bytes: payload };
                let declared = section.u32()? as usize;
                // Trust the section length, not the declared count.
                if declared > section.bytes.len() / 12 {
                    return Err(StateError::Truncated);
                }
                for _ in 0..declared {
                    let id = section.u32()?;
                    let bits = section.u64()?;
                    params.push((id, bits));
                }
            }
            TAG_EXTRA => extra = payload.to_vec(),
            other => unknown_sections.push((other, payload.to_vec())),
        }
    }

    // ---- Apply phase ----
    store.unknown_params.clear();
    store.unknown_sections = unknown_sections;
    let mut report = LoadReport {
        plugin_version,
        ..Default::default()
    };
    report.unknown_sections = store.unknown_sections.len();
    for (raw_id, bits) in params {
        let (id, value) = migrations.apply(plugin_version, raw_id, f64::from_bits(bits));
        match table.by_id(ParamId(id)) {
            Some(p) => {
                // Restored state must not fade in.
                p.restore(value);
                report.applied += 1;
            }
            None => {
                store.unknown_params.push((id, value.to_bits()));
                report.unknown_params += 1;
            }
        }
    }
    Ok((report, extra))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{FloatParam, Range, Smooth};

    struct T2(FloatParam, FloatParam);
    impl Params for T2 {
        fn count(&self) -> usize {
            2
        }
        fn by_index(&self, i: usize) -> Option<&dyn crate::params::AnyParam> {
            [&self.0 as &dyn crate::params::AnyParam, &self.1]
                .get(i)
                .copied()
        }
    }

    fn table() -> T2 {
        T2(
            FloatParam::new(ParamId(10), "drive", Range::linear(-12.0, 36.0)).default(0.0)
                .smooth(Smooth::Off),
            FloatParam::new(ParamId(11), "cutoff", Range::log(20.0, 20_000.0)).default(1200.0)
                .smooth(Smooth::Off),
        )
    }

    use crate::params::AnyParam;

    #[test]
    fn round_trip_is_bit_exact() {
        let a = table();
        (&a.0 as &dyn AnyParam).set_value(6.5);
        (&a.1 as &dyn AnyParam).set_value(632.4553);
        let chunk = save(&a, 3, b"hello", &StateStore::default());

        let b = table();
        let mut store = StateStore::default();
        let (report, extra) = load(&chunk, &b, &Migrations::default(), &mut store).unwrap();
        assert_eq!(report.applied, 2);
        assert_eq!(report.plugin_version, 3);
        assert_eq!(extra, b"hello");
        assert_eq!(AnyParam::value(&b.0).to_bits(), 6.5f64.to_bits());
        assert_eq!(AnyParam::value(&b.1).to_bits(), 632.4553f64.to_bits());
    }

    #[test]
    fn unknown_params_and_sections_survive_a_resave() {
        // "v9" chunk: two known params, one unknown param, one unknown
        // section — as written by a future version of the plugin.
        let future = table();
        (&future.0 as &dyn AnyParam).set_value(1.0);
        let mut future_store = StateStore::default();
        future_store.unknown_params.push((999, 0.777f64.to_bits()));
        future_store.unknown_sections.push((77, vec![1, 2, 3, 4]));
        let v9_chunk = save(&future, 9, &[], &future_store);

        // "v8" binary loads it: applies what it knows, keeps the rest.
        let old = table();
        let mut store = StateStore::default();
        let (report, _) = load(&v9_chunk, &old, &Migrations::default(), &mut store).unwrap();
        assert_eq!(report.applied, 2);
        assert_eq!(report.unknown_params, 1);
        assert_eq!(report.unknown_sections, 1);

        // …and re-emits them intact on save.
        let resaved = save(&old, 8, &[], &store);
        let check = table();
        let mut check_store = StateStore::default();
        let (r2, _) = load(&resaved, &check, &Migrations::default(), &mut check_store).unwrap();
        assert_eq!(r2.unknown_params, 1);
        assert_eq!(check_store.unknown_params, vec![(999, 0.777f64.to_bits())]);
        assert_eq!(check_store.unknown_sections, vec![(77, vec![1, 2, 3, 4])]);
    }

    #[test]
    fn migrations_rename_then_rescale() {
        let a = table();
        (&a.0 as &dyn AnyParam).set_value(12.0);
        let chunk = save(&a, 1, &[], &StateStore::default());

        // v2 renamed 10 → 20… so build a table with the NEW id, plus a
        // rescale that halves the stored value.
        struct V2(FloatParam);
        impl Params for V2 {
            fn count(&self) -> usize {
                1
            }
            fn by_index(&self, i: usize) -> Option<&dyn crate::params::AnyParam> {
                (i == 0).then_some(&self.0 as &dyn crate::params::AnyParam)
            }
        }
        let v2 = V2(
            FloatParam::new(ParamId(20), "drive", Range::linear(-12.0, 36.0)).default(0.0)
                .smooth(Smooth::Off),
        );
        let m = Migrations::default()
            .renamed(ParamId(10), ParamId(20))
            .rescaled(ParamId(20), |v| v * 0.5);
        let mut store = StateStore::default();
        let (report, _) = load(&chunk, &v2, &m, &mut store).unwrap();
        assert_eq!(report.applied, 1);
        assert_eq!(v2.0.value(), 6.0);
        // Old id 11 has no home in V2 → retained, not lost.
        assert_eq!(report.unknown_params, 1);
    }

    #[test]
    fn versioned_rescale_only_changes_state_with_old_plain_units() {
        let old_table = table();
        (&old_table.0 as &dyn AnyParam).set_value(2.0);
        let oldest_chunk = save(&old_table, 0, &[], &StateStore::default());
        let old_chunk = save(&old_table, 1, &[], &StateStore::default());
        let current_chunk = save(&old_table, 2, &[], &StateStore::default());

        let migrations = Migrations::default()
            .rescaled_before(1, ParamId(10), |value| value + 1.0)
            .rescaled_before(2, ParamId(10), |value| value * 2.0);

        let loaded_oldest_table = table();
        load(
            &oldest_chunk,
            &loaded_oldest_table,
            &migrations,
            &mut StateStore::default(),
        )
        .unwrap();
        assert_eq!(loaded_oldest_table.0.value(), 6.0);

        let loaded_old_table = table();
        load(
            &old_chunk,
            &loaded_old_table,
            &migrations,
            &mut StateStore::default(),
        )
        .unwrap();
        assert_eq!(loaded_old_table.0.value(), 4.0);

        let loaded_current_table = table();
        load(
            &current_chunk,
            &loaded_current_table,
            &migrations,
            &mut StateStore::default(),
        )
        .unwrap();
        assert_eq!(loaded_current_table.0.value(), 2.0);
    }

    #[test]
    fn corrupt_chunks_error_cleanly_and_apply_nothing() {
        let t = table();
        let mut store = StateStore::default();
        let m = Migrations::default();
        assert!(matches!(
            load(b"", &t, &m, &mut store),
            Err(StateError::TooShort)
        ));
        assert!(matches!(
            load(b"NOPE00000000", &t, &m, &mut store),
            Err(StateError::BadMagic)
        ));
        // Oversized section length: clean error, no giant allocation.
        let mut evil = Vec::new();
        evil.extend_from_slice(&MAGIC);
        evil.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        evil.extend_from_slice(&1u32.to_le_bytes());
        evil.extend_from_slice(&TAG_EXTRA.to_le_bytes());
        evil.extend_from_slice(&u32::MAX.to_le_bytes()); // 4 GiB claim
        assert!(matches!(
            load(&evil, &t, &m, &mut store),
            Err(StateError::Oversized { .. })
        ));
        // Format from the future is refused, not misread.
        let mut future = save(&t, 1, &[], &StateStore::default());
        future[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            load(&future, &t, &m, &mut store),
            Err(StateError::UnsupportedFormat { found: 99 })
        ));
        // A clean error applies nothing: table keeps its values.
        assert_eq!(t.0.value(), 0.0);
    }

    /// Mutational fuzz, deterministic: every mutant of a valid chunk
    /// either loads or errors — never panics, never over-allocates.
    /// (In-tree stand-in for a cargo-fuzz target; Miri runs a reduced
    /// count.)
    #[test]
    fn mutational_fuzz_never_panics() {
        let t = table();
        let valid = save(&t, 1, b"extra-bytes", &StateStore::default());
        let iters = if cfg!(miri) { 40 } else { 50_000 };
        let mut rng: u64 = 0x9E3779B97F4A7C15;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        for _ in 0..iters {
            let mut mutant = valid.clone();
            // 1–4 byte mutations + occasional truncation.
            for _ in 0..=(next() % 4) {
                let i = (next() as usize) % mutant.len();
                mutant[i] = (next() & 0xff) as u8;
            }
            if next() % 4 == 0 {
                mutant.truncate((next() as usize) % (mutant.len() + 1));
            }
            let fresh = table();
            let mut store = StateStore::default();
            let _ = load(&mutant, &fresh, &Migrations::default(), &mut store);
        }
    }
}
