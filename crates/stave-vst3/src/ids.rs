//! Shell↔ABI glue: the engine-anchored identity trait and the
//! blanket that lifts it onto `StavePlugin<E>` (orphan-rule design —
//! export macros implement `Vst3Ids` on the project's LOCAL engine).

use crate::sys::{IAudioProcessorVtbl, IComponentVtbl, IEditControllerVtbl, Tuid};
use crate::wrapper::Vst3Export;

/// Per-plugin VST3 identity + vtable statics, implemented by the
/// export macro on the ENGINE type (local to the project, so the
/// orphan rule is satisfied); the shell picks everything up through
/// the blanket impl below.
pub trait Vst3Ids {
    const COMPONENT_CID: Tuid;
    const CONTROLLER_CID: Tuid;
    fn component_vtbl() -> *const IComponentVtbl;
    fn processor_vtbl() -> *const IAudioProcessorVtbl;
    fn controller_vtbl() -> *const IEditControllerVtbl;
}

impl<E> Vst3Export for stave_core::StavePlugin<E>
where
    E: stave_core::Spec + Vst3Ids,
{
    const COMPONENT_CID: Tuid = E::COMPONENT_CID;
    const CONTROLLER_CID: Tuid = E::CONTROLLER_CID;
    fn component_vtbl() -> *const IComponentVtbl {
        E::component_vtbl()
    }
    fn processor_vtbl() -> *const IAudioProcessorVtbl {
        E::processor_vtbl()
    }
    fn controller_vtbl() -> *const IEditControllerVtbl {
        E::controller_vtbl()
    }
}
