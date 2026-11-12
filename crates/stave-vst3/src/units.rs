//! Parameter folders as VST3 units: the root unit plus one unit per
//! parameter group, in table order. The root carries the factory-preset
//! program list when there is one.

use core::sync::atomic::{AtomicI32, Ordering};

use stave_core::params::Params;

use crate::sys::{
    ProgramListId, UnitId, UnitInfo, NO_PARENT_UNIT_ID, NO_PROGRAM_LIST_ID, ROOT_UNIT_ID,
};
use crate::wire_strings::write_utf16_name;

pub(crate) struct Units {
    groups: Vec<&'static str>,
    /// The host's last `selectUnit` (UI focus only; no DSP meaning).
    selected: AtomicI32,
}

impl Units {
    pub fn new(table: &dyn Params) -> Self {
        let mut groups: Vec<&'static str> = Vec::new();
        for group in (0..table.count())
            .filter_map(|i| table.by_index(i))
            .map(|p| p.meta().group)
        {
            if !group.is_empty() && !groups.contains(&group) {
                groups.push(group);
            }
        }
        Self {
            groups,
            selected: AtomicI32::new(ROOT_UNIT_ID),
        }
    }

    pub fn count(&self) -> usize {
        1 + self.groups.len()
    }

    pub fn selected(&self) -> UnitId {
        self.selected.load(Ordering::Relaxed)
    }

    /// `false` for an id that names no unit.
    pub fn select(&self, id: UnitId) -> bool {
        let known = usize::try_from(id).is_ok_and(|i| i < self.count());
        if known {
            self.selected.store(id, Ordering::Relaxed);
        }
        known
    }

    /// The unit a parameter of `group` lives in; ungrouped → root.
    pub fn unit_of(&self, group: &str) -> UnitId {
        self.groups
            .iter()
            .position(|g| *g == group)
            .map_or(ROOT_UNIT_ID, |i| i as UnitId + 1)
    }

    /// `false` past the last unit.
    pub fn info(
        &self,
        index: usize,
        root_programs: Option<ProgramListId>,
        dest: &mut UnitInfo,
    ) -> bool {
        if index == 0 {
            dest.id = ROOT_UNIT_ID;
            dest.parent_unit_id = NO_PARENT_UNIT_ID;
            write_utf16_name(&mut dest.name, "Root");
            dest.program_list_id = root_programs.unwrap_or(NO_PROGRAM_LIST_ID);
            return true;
        }
        let Some(group) = self.groups.get(index - 1) else {
            return false;
        };
        dest.id = index as UnitId;
        dest.parent_unit_id = ROOT_UNIT_ID;
        write_utf16_name(&mut dest.name, group);
        dest.program_list_id = NO_PROGRAM_LIST_ID;
        true
    }
}
