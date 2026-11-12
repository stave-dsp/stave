//! Pure model for Stave-owned select controls.
//!
//! The Cocoa overlay lives in `dropdown.rs`; this file owns sources,
//! catalog nodes, editor state, and hierarchical preset-path expansion.

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::objc::Id;
use crate::view::PluginEditorView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectSource {
    Control(usize),
    Presets,
}

/// One row in one level of a select catalog. Children are shown in a
/// separate cascading panel; they are never flattened into their parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectNode {
    pub(crate) title: String,
    pub(crate) selection: Option<usize>,
    pub(crate) children: Vec<SelectNode>,
}

impl SelectNode {
    pub(crate) fn leaf(title: String, selection: usize) -> Self {
        Self {
            title,
            selection: Some(selection),
            children: Vec::new(),
        }
    }

    pub(crate) fn branch(title: String) -> Self {
        Self {
            title,
            selection: None,
            children: Vec::new(),
        }
    }
}

pub(crate) struct SelectField {
    pub(crate) source: SelectSource,
    pub(crate) view: Id,
    pub(crate) label: Id,
    pub(crate) last_control_value: u64,
}

pub(crate) struct SelectPanel {
    pub(crate) menu: Id,
    pub(crate) nodes: Vec<SelectNode>,
    pub(crate) labels: Vec<Id>,
    pub(crate) hover: Option<usize>,
    pub(crate) scroll_offset: usize,
    pub(crate) visible_rows: usize,
}

pub(crate) struct OpenSelectMenu {
    pub(crate) scrim: Id,
    pub(crate) source: SelectSource,
    /// Root panel first, then one panel for each opened branch.
    pub(crate) panels: Vec<SelectPanel>,
}

/// All custom-select state owned by one editor. Only one cascading menu
/// may be open at once, regardless of its source.
pub(crate) struct DropdownState {
    pub(crate) host: Id,
    pub(crate) fields: Vec<SelectField>,
    pub(crate) open: Option<OpenSelectMenu>,
    pub(crate) selected_preset: usize,
}

impl Default for DropdownState {
    fn default() -> Self {
        Self {
            host: core::ptr::null_mut(),
            fields: Vec::new(),
            open: None,
            selected_preset: 0,
        }
    }
}

pub(crate) fn selected_control_step(view: &PluginEditorView, control_index: usize) -> usize {
    let step_count = (view.param(control_index).step_count() as usize).max(1);
    (view.plain(control_index) as usize).min(step_count - 1)
}

pub(crate) fn control_step_title(
    view: &PluginEditorView,
    control_index: usize,
    step: usize,
) -> String {
    let name = view
        .param(control_index)
        .step_name(step as u32)
        .unwrap_or("");
    if name.is_empty() {
        alloc::format!("{step}")
    } else {
        name.to_uppercase()
    }
}

pub(crate) fn preset_leaf_title(name: &str) -> String {
    name.split('/')
        .map(str::trim)
        .rfind(|part| !part.is_empty())
        .unwrap_or("UNTITLED")
        .to_uppercase()
}

pub(crate) fn field_title(view: &PluginEditorView, source: SelectSource) -> String {
    match source {
        SelectSource::Control(control_index) => control_step_title(
            view,
            control_index,
            selected_control_step(view, control_index),
        ),
        SelectSource::Presets => view
            .presets
            .get(view.dropdowns.selected_preset)
            .map(|(name, _)| preset_leaf_title(name))
            .unwrap_or_else(|| "INIT".to_string()),
    }
}

fn insert_preset_path(nodes: &mut Vec<SelectNode>, path: &[&str], preset_index: usize) {
    let Some(first) = path.first() else {
        return;
    };
    let title = first.trim().to_uppercase();
    if title.is_empty() {
        return;
    }
    let node_index = nodes
        .iter()
        .position(|node| node.title == title)
        .unwrap_or_else(|| {
            nodes.push(SelectNode::branch(title.clone()));
            nodes.len() - 1
        });
    let node = &mut nodes[node_index];
    if path.len() == 1 {
        node.selection = Some(preset_index);
    } else {
        insert_preset_path(&mut node.children, &path[1..], preset_index);
    }
}

fn normalize_branch_presets(nodes: &mut [SelectNode]) {
    for node in nodes {
        normalize_branch_presets(&mut node.children);
        if !node.children.is_empty() {
            if let Some(selection) = node.selection.take() {
                node.children
                    .insert(0, SelectNode::leaf("DEFAULT".to_string(), selection));
            }
        }
    }
}

fn preset_nodes_from_names<'a>(
    names: impl IntoIterator<Item = (usize, &'a str)>,
) -> Vec<SelectNode> {
    let mut nodes = Vec::new();
    for (preset_index, name) in names {
        let path: Vec<&str> = name
            .split('/')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        if path.is_empty() {
            insert_preset_path(&mut nodes, &["UNTITLED"], preset_index);
        } else {
            insert_preset_path(&mut nodes, &path, preset_index);
        }
    }
    normalize_branch_presets(&mut nodes);
    if nodes.is_empty() {
        nodes.push(SelectNode::branch("INIT".to_string()));
    }
    nodes
}

pub(crate) fn nodes_for_source(view: &PluginEditorView, source: SelectSource) -> Vec<SelectNode> {
    match source {
        SelectSource::Control(control_index) => {
            let step_count = (view.param(control_index).step_count() as usize).max(1);
            (0..step_count)
                .map(|step| SelectNode::leaf(control_step_title(view, control_index, step), step))
                .collect()
        }
        SelectSource::Presets => preset_nodes_from_names(
            view.presets
                .iter()
                .enumerate()
                .map(|(preset_index, (name, _))| (preset_index, name.as_str())),
        ),
    }
}

pub(crate) fn selected_value(view: &PluginEditorView, source: SelectSource) -> usize {
    match source {
        SelectSource::Control(control_index) => selected_control_step(view, control_index),
        SelectSource::Presets => view.dropdowns.selected_preset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_paths_remain_a_tree() {
        let nodes = preset_nodes_from_names([
            (0, "FACTORY / CLEAN / WARM"),
            (1, "FACTORY / DRIVEN / PUSHED"),
            (2, "ARTIST / RYAN / NIGHT DRIVE"),
        ]);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].title, "FACTORY");
        assert_eq!(nodes[0].children.len(), 2);
        assert_eq!(nodes[0].children[0].title, "CLEAN");
        assert_eq!(nodes[0].children[0].children[0].title, "WARM");
        assert_eq!(nodes[0].children[0].children[0].selection, Some(0));
        assert_eq!(nodes[1].title, "ARTIST");
    }

    #[test]
    fn flat_presets_remain_selectable_root_nodes() {
        let nodes = preset_nodes_from_names([(0, "WARM"), (1, "PUSHED")]);
        assert_eq!(nodes[0].selection, Some(0));
        assert!(nodes[0].children.is_empty());
        assert_eq!(nodes[1].selection, Some(1));
    }

    #[test]
    fn closed_field_uses_only_the_leaf_name() {
        assert_eq!(preset_leaf_title("FACTORY / CLEAN / WARM"), "WARM");
    }
}
