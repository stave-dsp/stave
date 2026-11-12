//! ui! markup goldens: the XML surface produces exactly the trees the
//! constructors build, and layout solves identically. Runs headless.

use stave_ui::{solve, ui, Dim, FixedMetrics, Shape, TextStyle};

const M: FixedMetrics = FixedMetrics {
    advance: 0.5,
    line: 1.0,
};

#[test]
fn markup_builds_the_same_tree_as_constructors() {
    let cap = TextStyle {
        size: 10.0,
        ..Default::default()
    };
    let a: stave_ui::Node<()> = ui! {
        <Column w=200 pad=10 gap=4>
            <Text class={cap}>"DYNAMICS"</Text>
            <Row gap=8 align=Center>
                <Shape kind=RRect w=3 h=100 radius=1.5/>
                <Spacer/>
                <Glyph name="power" w=16 h=16/>
            </Row>
            <Grid cols=4 gap=8>
                {stave_ui::widget(()).size(40.0, 40.0)}
                {stave_ui::widget(()).size(40.0, 40.0)}
            </Grid>
        </Column>
    };
    let b: stave_ui::Node<()> = stave_ui::column(vec![
        stave_ui::text("DYNAMICS").text_style(cap),
        stave_ui::row(vec![
            stave_ui::shape(Shape::RRect)
                .width(Dim::Px(3.0))
                .height(Dim::Px(100.0))
                .radius(1.5),
            stave_ui::spacer(),
            stave_ui::glyph("power")
                .width(Dim::Px(16.0))
                .height(Dim::Px(16.0)),
        ])
        .gap(8.0)
        .align(stave_ui::Align::Center),
        stave_ui::grid(
            4,
            vec![
                stave_ui::widget(()).size(40.0, 40.0),
                stave_ui::widget(()).size(40.0, 40.0),
            ],
        )
        .gap(8.0),
    ])
    .width(Dim::Px(200.0))
    .pad(10.0)
    .gap(4.0);
    assert_eq!(a, b, "markup and constructors must be the SAME tree");
    let la = solve(&a, (300.0, 300.0), &M);
    let lb = solve(&b, (300.0, 300.0), &M);
    assert_eq!(la, lb);
}

struct Power {
    param: u32,
}
impl Power {
    fn node(self) -> stave_ui::Node<u32> {
        stave_ui::widget(self.param)
    }
}

#[test]
fn components_expand_to_struct_literal_plus_node() {
    let n: stave_ui::Node<u32> = ui! {
        <Row gap=6>
            <Power param={41 + 1} w=60 h=22/>
        </Row>
    };
    assert_eq!(n.children.len(), 1);
    assert_eq!(n.children[0], stave_ui::widget(42u32).size(60.0, 22.0));
}
