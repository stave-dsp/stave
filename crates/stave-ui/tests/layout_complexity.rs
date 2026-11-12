//! `solve` is linear in the leaves and polynomial in the depth: each
//! leaf is measured once per solve, and a deeply nested face lays out
//! in a blink (it took minutes when every level sized its children
//! twice over).

use std::cell::Cell;
use std::time::Instant;

use stave_ui::{column, row, solve, spacer, stack, widget, Dim, Measure, Node};

/// Counts how often layout asks a leaf its size.
struct Counting<'a>(&'a Cell<usize>);

impl Measure<u32> for Counting<'_> {
    fn size(&self, leaf: &u32) -> (f64, f64) {
        self.0.set(self.0.get() + 1);
        (20.0 + f64::from(*leaf % 7), 10.0 + f64::from(*leaf % 5))
    }
}

/// A face-like tree `depth` levels deep: at each level a padded box
/// (alternating row and column, a stack every third) holding a leaf, a
/// spacer, a Fill column and the next level.
fn nested(depth: u32) -> Node<u32> {
    let mut inner = widget(depth).width(Dim::Fill(1.0));
    for d in (0..depth).rev() {
        let kids = vec![
            widget(d),
            spacer(),
            column(vec![widget(d + 100), widget(d + 200)]).width(Dim::Fill(1.0)).gap(2.0),
            inner,
        ];
        inner = match d % 3 {
            0 => row(kids).gap(4.0),
            1 => column(kids).gap(3.0),
            _ => stack(kids),
        }
        .pad(2.0)
        .margin(1.0);
    }
    inner
}

fn leaves(node: &Node<u32>) -> usize {
    usize::from(matches!(node.kind, stave_ui::Kind::Widget(_))) + node.children.iter().map(leaves).sum::<usize>()
}

#[test]
fn a_solve_measures_each_leaf_once() {
    for depth in [1, 8, 40] {
        let tree = nested(depth);
        let calls = Cell::new(0);
        let _ = solve(&tree, (0.0, 0.0), &Counting(&calls));
        let _ = solve(&tree, (800.0, 600.0), &Counting(&calls));
        assert_eq!(calls.get(), 2 * leaves(&tree), "depth {depth}");
    }
}

#[test]
fn a_deep_face_lays_out_in_a_blink() {
    let tree = nested(64);
    let calls = Cell::new(0);
    let t = Instant::now();
    let _ = solve(&tree, (900.0, 700.0), &Counting(&calls));
    assert!(t.elapsed().as_secs_f64() < 1.0, "64 levels took {:?}", t.elapsed());
}
