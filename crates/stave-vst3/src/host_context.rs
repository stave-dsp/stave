//! Host timeline/context plumbing: the SDK ≥3.7
//! IProcessContextRequirements declaration and the validity-gated
//! ProcessContext → Transport translation (one concept: what we ask
//! the host for, and how we read what it gives us).

use stave_core::Transport;

use crate::sys::*;
use crate::wrapper::{shared_add_ref, shared_query_interface, shared_release, Vst3Export};

/// SDK ≥3.7 contract: declare the ProcessContext fields we consume
/// (see `translate_transport`). Hosts may skip computing the rest.
pub(crate) unsafe extern "C" fn pcr_get(_this: *mut FUnknown) -> u32 {
    PCR_NEED_TEMPO | PCR_NEED_PROJECT_TIME_MUSIC | PCR_NEED_TRANSPORT_STATE
}

pub(crate) const fn build_pcr_vtbl<P: Vst3Export>() -> IProcessContextRequirementsVtbl {
    IProcessContextRequirementsVtbl {
        query_interface: shared_query_interface::<P>,
        add_ref: shared_add_ref::<P>,
        release: shared_release::<P>,
        get_process_context_requirements: pcr_get,
    }
}

/// Map a (validity-gated) host ProcessContext into our Transport.
pub(crate) fn translate_transport(c: &ProcessContext) -> Transport {
    Transport {
        playing: c.state & PC_PLAYING != 0,
        tempo_bpm: (c.state & PC_TEMPO_VALID != 0).then_some(c.tempo),
        ppq_position: (c.state & PC_PROJECT_TIME_MUSIC_VALID != 0).then_some(c.project_time_music),
        song_pos_seconds: (c.sample_rate > 0.0)
            .then(|| c.project_time_samples as f64 / c.sample_rate),
    }
}
