//! Snapshot layout tests — the solver's geometry pinned headless (any
//! OS, no display). If a rect moves, a check fails; screenshots only
//! guard PAINT, never placement.

use stave_ui::{column, row, solve, spacer, stack, widget, Align, Dim, Justify};

fn rects(l: &stave_ui::Layout) -> Vec<(f64, f64, f64, f64)> {
    l.children
        .iter()
        .map(|c| (c.rect.x, c.rect.y, c.rect.w, c.rect.h))
        .collect()
}

#[test]
fn row_with_gap_padding_and_fixed_children() {
    let tree = row::<()>(vec![
        widget(()).size(40.0, 40.0),
        widget(()).size(40.0, 40.0),
        widget(()).size(40.0, 40.0),
    ])
    .pad(10.0)
    .gap(8.0)
    .size(200.0, 60.0);
    let l = solve(&tree, (400.0, 400.0));
    assert_eq!(l.rect.w, 200.0);
    assert_eq!(
        rects(&l),
        vec![
            (10.0, 10.0, 40.0, 40.0),
            (58.0, 10.0, 40.0, 40.0),
            (106.0, 10.0, 40.0, 40.0)
        ]
    );
}

#[test]
fn fill_shares_the_remainder_by_weight() {
    let tree = row::<()>(vec![
        widget(()).size(50.0, 20.0),
        widget(()).width(Dim::Fill(1.0)).height(Dim::Px(20.0)),
        widget(()).width(Dim::Fill(3.0)).height(Dim::Px(20.0)),
    ])
    .gap(10.0)
    .size(300.0, 20.0);
    let l = solve(&tree, (300.0, 300.0));
    // Remainder = 300 − (50 + 2 gaps) = 230; weights 1:3 → 57.5 / 172.5.
    assert_eq!(
        rects(&l),
        vec![
            (0.0, 0.0, 50.0, 20.0),
            (60.0, 0.0, 57.5, 20.0),
            (127.5, 0.0, 172.5, 20.0)
        ]
    );
}

#[test]
fn spacer_is_the_toolbar_spring() {
    let tree = row::<()>(vec![
        widget(()).size(30.0, 10.0),
        spacer(),
        widget(()).size(30.0, 10.0),
    ])
    .size(200.0, 10.0);
    let l = solve(&tree, (200.0, 200.0));
    assert_eq!(rects(&l)[2].0, 170.0, "right item flush right");
}

#[test]
fn wrap_flows_to_the_next_line_with_cross_gap() {
    let tree = row::<()>(vec![
        widget(()).size(60.0, 20.0),
        widget(()).size(60.0, 20.0),
        widget(()).size(60.0, 20.0),
    ])
    .gap(10.0)
    .wrap()
    .size(150.0, 100.0);
    let l = solve(&tree, (400.0, 400.0));
    let r = rects(&l);
    assert_eq!(r[0], (0.0, 0.0, 60.0, 20.0));
    assert_eq!(r[1], (70.0, 0.0, 60.0, 20.0));
    assert_eq!(r[2], (0.0, 30.0, 60.0, 20.0), "wrapped, one cross gap down");
}

#[test]
fn intrinsic_height_wraps_at_the_resolved_width() {
    // A fixed-width wrapping row inside an Auto-height column: the
    // column must measure the row at ITS width (150 → two lines), not
    // at the parent's avail (400 → one line). Regression: smear's
    // window height solved to a single card row.
    let tree = column::<()>(vec![row(vec![
        widget(()).size(60.0, 20.0),
        widget(()).size(60.0, 20.0),
        widget(()).size(60.0, 20.0),
    ])
    .gap(10.0)
    .wrap()
    .width(Dim::Px(150.0))]);
    let l = solve(&tree, (400.0, 400.0));
    assert_eq!(l.rect.h, 50.0, "two lines of 20 + one 10 cross gap");
}

#[test]
fn justify_and_align_center_a_child() {
    let tree = row::<()>(vec![widget(()).size(50.0, 20.0)])
        .justify(Justify::Center)
        .align(Align::Center)
        .size(200.0, 100.0);
    let l = solve(&tree, (200.0, 100.0));
    assert_eq!(rects(&l), vec![(75.0, 40.0, 50.0, 20.0)]);
}

#[test]
fn space_between_spreads_the_footer() {
    let tree = row::<()>(vec![
        widget(()).size(40.0, 10.0),
        widget(()).size(40.0, 10.0),
        widget(()).size(40.0, 10.0),
    ])
    .justify(Justify::SpaceBetween)
    .size(240.0, 10.0);
    let l = solve(&tree, (240.0, 240.0));
    let r = rects(&l);
    assert_eq!((r[0].0, r[1].0, r[2].0), (0.0, 100.0, 200.0));
}

#[test]
fn padding_insets_content() {
    let tree = column::<()>(vec![widget(()).size(10.0, 10.0)]).pad(6.0);
    let l = solve(&tree, (100.0, 100.0));
    assert_eq!(
        l.rect.w,
        10.0 + 2.0 * 6.0,
        "auto width = content + padding"
    );
    assert_eq!(rects(&l), vec![(6.0, 6.0, 10.0, 10.0)]);
}

#[test]
fn stack_overlays_and_centers() {
    let tree = stack::<()>(vec![
        widget(()).width(Dim::Fill(1.0)).height(Dim::Fill(1.0)),
        widget(()).size(20.0, 20.0),
    ])
    .justify(Justify::Center)
    .align(Align::Center)
    .size(100.0, 100.0);
    let l = solve(&tree, (100.0, 100.0));
    let r = rects(&l);
    assert_eq!(r[0], (0.0, 0.0, 100.0, 100.0), "fill covers the box");
    assert_eq!(r[1], (40.0, 40.0, 20.0, 20.0), "overlay centered");
}

#[test]
fn margins_space_siblings_and_offset_position() {
    let tree = row::<()>(vec![
        widget(()).size(30.0, 30.0).margin(5.0),
        widget(()).size(30.0, 30.0),
    ])
    .size(200.0, 40.0);
    let l = solve(&tree, (200.0, 200.0));
    assert_eq!(
        rects(&l),
        vec![(5.0, 5.0, 30.0, 30.0), (40.0, 0.0, 30.0, 30.0)]
    );
}

#[test]
fn stretch_grows_auto_cross_children() {
    let tree = row::<()>(vec![
        widget(()).size(30.0, 60.0),
        column::<()>(vec![]).width(Dim::Px(30.0)),
    ])
    .align(Align::Stretch)
    .size(100.0, 60.0);
    let l = solve(&tree, (100.0, 60.0));
    assert_eq!(
        rects(&l)[1].3,
        60.0,
        "auto-height sibling stretched to line"
    );
}

#[test]
fn a_card_shaped_composition_lays_out() {
    // card = column(title, 4×3 knob grid (wrap), toggles row) —
    // proving the vocabulary expresses it.
    let knob = || widget(()).size(40.0, 52.0);
    let card = column::<()>(vec![
        widget(()).size(16.5, 11.0),
        row((0..11).map(|_| knob()).collect())
            .gap(8.0)
            .wrap()
            .width(Dim::Px(184.0)),
        row(vec![
            widget(()).size(80.0, 22.0),
            widget(()).size(80.0, 22.0),
        ])
        .justify(Justify::SpaceBetween),
    ])
    .pad(12.0)
    .gap(10.0);
    let l = solve(&card, (400.0, 600.0));
    assert_eq!(l.children.len(), 3);
    let grid = &l.children[1];
    assert_eq!(grid.children.len(), 11);
    // 184 content width fits 4 knobs (4×40 + 3×8 = 184) → 3 rows.
    assert_eq!(grid.children[4].rect.y - grid.children[0].rect.y, 60.0);
    assert_eq!(grid.children[8].rect.y - grid.children[4].rect.y, 60.0);
}

#[test]
fn grid_flows_row_major_with_equal_columns() {
    // The knob-grid case: 11 cells, 4 columns, both-axis gap.
    let tree = stave_ui::grid::<()>(4, (0..11).map(|_| widget(()).size(80.0, 40.0)).collect())
        .gap(8.0)
        .size(360.0, 200.0);
    let l = solve(&tree, (360.0, 200.0));
    let r = rects(&l);
    // col width = (360 − 3·8)/4 = 84; cells keep their 80 within it.
    assert_eq!(r[0], (0.0, 0.0, 80.0, 40.0));
    assert_eq!(r[1].0, 92.0, "second column at col_w+gap");
    assert_eq!(r[4].1, 48.0, "second row below tallest+gap");
    assert_eq!(r[8].1, 96.0, "third row");
    assert_eq!(r[10].0, 2.0 * 92.0, "11th cell in third column");
}
