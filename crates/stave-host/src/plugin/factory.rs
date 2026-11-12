//! The plugin factory: what classes a module offers, and creating
//! them. Hosts must pick the AUDIO MODULE class by category — factories
//! routinely list the controller (or several plugins) first.

use std::ffi::c_void;

use stave_vst3::sys::{
    IPluginFactoryVtbl, PClassInfo, PClassInfo2, Tresult, Tuid, IID_IPLUGIN_FACTORY2, K_RESULT_OK,
};

use super::com::{c_str_z, Com};
use super::module::EntryPoints;
use super::HostError;

/// The category of a class hosts instantiate as a plugin.
pub const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

/// One class the factory advertises.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ClassInfo {
    pub index: i32,
    pub name: String,
    pub category: String,
    /// IPluginFactory2 extras (empty when the factory is v1-only).
    pub sub_categories: String,
    pub vendor: String,
    pub version: String,
    pub sdk_version: String,
    #[serde(skip)]
    pub cid: Tuid,
}

pub(crate) struct Factory {
    com: Com<IPluginFactoryVtbl>,
    /// The same object viewed as IPluginFactory2, when supported.
    v2: Option<Com<IPluginFactoryVtbl>>,
}

impl Factory {
    pub fn new(entry: &EntryPoints) -> Result<Self, HostError> {
        // SAFETY: GetPluginFactory hands the host one reference to an
        // IPluginFactory (which `Com` releases).
        let com: Com<IPluginFactoryVtbl> =
            unsafe { Com::adopt((entry.get_factory)().cast()) }.ok_or(HostError::FactoryNull)?;
        // SAFETY: IPluginFactory2 extends the v1 layout; only its v2
        // slots are read through `v2`.
        let v2 = unsafe { com.query(&IID_IPLUGIN_FACTORY2) };
        Ok(Self { com, v2 })
    }

    /// The factory's vendor string (`PFactoryInfo.vendor`).
    pub fn vendor(&self) -> Option<String> {
        // SAFETY: all-zero is a valid `PFactoryInfo` (char arrays, an
        // integer).
        let mut info = unsafe { std::mem::zeroed::<stave_vst3::sys::PFactoryInfo>() };
        // SAFETY: the factory's own method on the live factory.
        let r = unsafe { (self.com.vtbl().get_factory_info)(self.com.as_ptr(), &mut info) };
        (r == K_RESULT_OK)
            .then(|| c_str_z(&info.vendor))
            .filter(|v| !v.is_empty())
    }

    pub fn classes(&self) -> Vec<ClassInfo> {
        // SAFETY: the factory's own method on the live factory.
        let n = unsafe { (self.com.vtbl().count_classes)(self.com.as_ptr()) };
        (0..n).filter_map(|i| self.class(i)).collect()
    }

    fn class(&self, index: i32) -> Option<ClassInfo> {
        if let Some(v2) = &self.v2 {
            // SAFETY: all-zero is a valid `PClassInfo2` (a byte id,
            // integers, char arrays).
            let mut c = unsafe { std::mem::zeroed::<PClassInfo2>() };
            // SAFETY: the factory's own method on the live factory.
            if unsafe { (v2.vtbl().get_class_info_2)(v2.as_ptr(), index, &mut c) } == K_RESULT_OK {
                return Some(ClassInfo {
                    index,
                    name: c_str_z(&c.name),
                    category: c_str_z(&c.category),
                    sub_categories: c_str_z(&c.sub_categories),
                    vendor: c_str_z(&c.vendor),
                    version: c_str_z(&c.version),
                    sdk_version: c_str_z(&c.sdk_version),
                    cid: c.cid,
                });
            }
        }
        // SAFETY: all-zero is a valid `PClassInfo`.
        let mut c = unsafe { std::mem::zeroed::<PClassInfo>() };
        // SAFETY: the factory's own method on the live factory.
        let r = unsafe { (self.com.vtbl().get_class_info)(self.com.as_ptr(), index, &mut c) };
        (r == K_RESULT_OK).then(|| ClassInfo {
            index,
            name: c_str_z(&c.name),
            category: c_str_z(&c.category),
            sub_categories: String::new(),
            vendor: String::new(),
            version: String::new(),
            sdk_version: String::new(),
            cid: c.cid,
        })
    }

    /// `createInstance(cid, iid)`, viewed as `V`.
    ///
    /// # Safety
    /// `V` must be the vtable layout of interface `iid`.
    pub unsafe fn create<V>(&self, cid: &Tuid, iid: &Tuid) -> Result<Com<V>, Tresult> {
        let mut obj: *mut c_void = std::ptr::null_mut();
        // SAFETY: the factory's own method on the live factory; the TUIDs
        // are 16 bytes, as `FIDString` expects.
        let r = unsafe {
            (self.com.vtbl().create_instance)(
                self.com.as_ptr(),
                cid.as_ptr().cast(),
                iid.as_ptr().cast(),
                &mut obj,
            )
        };
        if r != K_RESULT_OK {
            return Err(r);
        }
        // SAFETY: success hands over one reference to an object
        // implementing `iid`, laid out as `V` (this function's contract).
        unsafe { Com::adopt(obj) }.ok_or(stave_vst3::sys::K_NO_INTERFACE)
    }
}

/// The class to instantiate: the first audio module.
pub fn choose_audio_module(classes: &[ClassInfo]) -> Option<&ClassInfo> {
    classes.iter().find(|c| c.category == AUDIO_MODULE_CLASS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(index: i32, category: &str) -> ClassInfo {
        ClassInfo {
            index,
            name: format!("c{index}"),
            category: category.into(),
            sub_categories: String::new(),
            vendor: String::new(),
            version: String::new(),
            sdk_version: String::new(),
            cid: [index as u8; 16],
        }
    }

    #[test]
    fn skips_controller_classes_listed_first() {
        let classes = [
            class(0, "Component Controller Class"),
            class(1, AUDIO_MODULE_CLASS),
        ];
        assert_eq!(choose_audio_module(&classes).unwrap().index, 1);
    }

    #[test]
    fn none_when_no_audio_module() {
        assert!(choose_audio_module(&[class(0, "Plugin Compatibility Class")]).is_none());
        assert!(choose_audio_module(&[]).is_none());
    }
}
