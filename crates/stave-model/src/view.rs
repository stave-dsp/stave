//! An editor's lifetime against its instance's (VST3 `IPlugView`, S7):
//! the host creates a view from the controller, may attach and detach it
//! any number of times, and releases the controller and the view in
//! either order. The view's calls reach the instance, so the instance
//! must outlive every view — a host holding the view past the
//! controller (several do) must not see a use after free.
//!
//! [`ViewRef::Borrowed`] is the view as first written: a faked refcount
//! that never kept the instance. [`ViewRef::Counted`] is the shipping
//! view, holding one counted reference to the instance until its own
//! count reaches zero. The law: no call ever reaches a freed instance,
//! and once every reference is gone the instance is freed (no leak).

use crate::check::Model;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ViewRef {
    Borrowed,
    Counted,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    /// References to the instance: the host's controller reference plus,
    /// when counted, one per live view.
    pub instance_refs: u8,
    pub instance_freed: bool,
    pub host_has_controller: bool,
    /// The host's references to the view (0: none or released).
    pub view_refs: u8,
    pub view_created: bool,
    pub attached: bool,
    pub use_after_free: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CreateView,
    AddRefView,
    Attach,
    Detach,
    /// Any call into the view (resize, events, a timer tick).
    CallView,
    ReleaseView,
    ReleaseController,
}

pub struct View {
    pub refs: ViewRef,
}

impl View {
    fn drop_instance_ref(s: &mut State) {
        s.instance_refs -= 1;
        if s.instance_refs == 0 {
            s.instance_freed = true;
        }
    }
}

impl Model for View {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        vec![State {
            instance_refs: 1,
            instance_freed: false,
            host_has_controller: true,
            view_refs: 0,
            view_created: false,
            attached: false,
            use_after_free: false,
        }]
    }

    fn actions(&self, s: &State, out: &mut Vec<Action>) {
        use Action::*;
        if s.host_has_controller && !s.view_created {
            out.push(CreateView);
        }
        if s.view_refs > 0 {
            out.extend([AddRefView, CallView, ReleaseView]);
            out.push(if s.attached { Detach } else { Attach });
        }
        if s.host_has_controller {
            out.push(ReleaseController);
        }
    }

    fn next(&self, s: &State, a: &Action) -> Option<State> {
        let mut n = s.clone();
        match a {
            Action::CreateView if s.host_has_controller && !s.view_created => {
                n.view_created = true;
                n.view_refs = 1;
                if self.refs == ViewRef::Counted {
                    n.instance_refs += 1;
                }
            }
            Action::AddRefView if s.view_refs > 0 && s.view_refs < 3 => n.view_refs += 1,
            Action::Attach | Action::Detach | Action::CallView if s.view_refs > 0 => {
                n.use_after_free |= s.instance_freed;
                match a {
                    Action::Attach => n.attached = true,
                    Action::Detach => n.attached = false,
                    _ => {}
                }
            }
            Action::ReleaseView if s.view_refs > 0 => {
                n.view_refs -= 1;
                if n.view_refs == 0 {
                    // A host may release an attached view: it detaches.
                    n.attached = false;
                    if self.refs == ViewRef::Counted {
                        Self::drop_instance_ref(&mut n);
                    }
                }
            }
            Action::ReleaseController if s.host_has_controller => {
                n.host_has_controller = false;
                Self::drop_instance_ref(&mut n);
            }
            _ => return None,
        }
        Some(n)
    }

    fn invariant(&self, s: &State) -> Result<(), String> {
        if s.use_after_free {
            return Err("a view call reached a freed instance".into());
        }
        let unreferenced = !s.host_has_controller && s.view_refs == 0;
        if unreferenced && !s.instance_freed {
            return Err("every reference is gone but the instance lives (a leak)".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;

    #[test]
    fn a_borrowed_view_outliving_its_controller_is_a_use_after_free() {
        let cx = check(&View { refs: ViewRef::Borrowed }, 10_000).expect_err("S7");
        assert_eq!(cx.property, "a view call reached a freed instance", "{cx}");
    }

    #[test]
    fn a_counted_view_keeps_its_instance_and_frees_it_last() {
        check(&View { refs: ViewRef::Counted }, 10_000).expect("the shipping view");
    }
}
