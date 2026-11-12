//! The multiband editor as owned `ui!` markup: header, spectrum, three
//! band cards and the crossover footer, a pure function of params.

extern crate alloc;

use stave_core::editor_kit::{Knob, Plot, Power, Selector, Spectrum, Toggle};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim, Edges, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::params::{BandParams, MbParams, GR_PROBES};
use crate::saturation::SATURATIONS;
use crate::style::STYLES;

const CARD_W: f64 = 368.0;
const CARD_H: f64 = 372.0;

pub fn editor(p: &MbParams) -> UiNode {
    let [low, mid, high] = &p.bands;
    let spectrum = Spectrum {
        splits: &[&p.xover_low, &p.xover_high],
        thresholds: &[&low.threshold, &mid.threshold, &high.threshold],
        sc_hpfs: &[&low.sidechain_hpf, &mid.sidechain_hpf, &high.sidechain_hpf],
        gr_probes: &GR_PROBES,
    }
    .node();
    ui! {
        <Column w=1180 align=Center bg={tokens::BG_0}
                pad={Edges { t: 6.0, r: 20.0, b: 18.0, l: 20.0 }}>
            <Row w=Fill h=28 gap=8 align=Center pad={Edges { r: 4.0, ..Edges::ZERO }}>
                <Text class={ty::WORDMARK} w=78 h=20 tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION} w=220 h=16>"MULTIBAND"</Text>
                <Spacer/>
                <Text class={ty::TAGLINE} w=150 h=12 tag="bypass-tag">"3-BAND  DYNAMICS"</Text>
                <Power param={&p.bypass} w=24 h=24 margin={Edges { l: 10.0, ..Edges::ZERO }}/>
            </Row>
            <Shape kind=Line w=Fill h=1 bg={tokens::LINE}/>
            {spectrum.width(Dim::Fill(1.0)).height(Dim::Px(64.0)).margin_edges(Edges { t: 4.0, ..Edges::ZERO })}
            <Text class={ty::CAPTION} w=Fill h=10 margin={Edges { t: 8.0, ..Edges::ZERO }}>"DYNAMICS"</Text>
            <Row gap=18 margin={Edges { t: 6.0, ..Edges::ZERO }}>
                {band_card(0, "LOW", low)}
                {band_card(1, "MID", mid)}
                {band_card(2, "HIGH", high)}
            </Row>
            {crossover(&[&p.xover_low, &p.xover_high]).margin_edges(Edges { t: 20.0, ..Edges::ZERO })}
        </Column>
    }
}

/// Knob grid reading order: compression curve row (threshold, ratio,
/// knee, makeup), envelope row (attack, release, sc HPF, rms), tone row
/// (drive, sat mix, mix).
fn band_card(slot: usize, title: &'static str, b: &BandParams) -> UiNode {
    BandCard {
        title,
        slot,
        knobs: [
            &b.threshold,
            &b.ratio,
            &b.knee,
            &b.makeup,
            &b.attack,
            &b.release,
            &b.sidechain_hpf,
            &b.rms,
            &b.drive,
            &b.saturation_mix,
            &b.mix,
        ],
        solo: &b.solo,
        mute: &b.mute,
        sat: &b.saturation,
        style: &b.style,
        gr_probe: GR_PROBES[slot],
    }
    .node()
}

/// One band's card: title and voicing pills, the knob grid, the transfer
/// plot, solo/mute.
struct BandCard<'a> {
    title: &'static str,
    slot: usize,
    knobs: [&'a dyn AnyParam; 11],
    solo: &'a dyn AnyParam,
    mute: &'a dyn AnyParam,
    sat: &'a dyn AnyParam,
    style: &'a dyn AnyParam,
    gr_probe: &'static str,
}

impl BandCard<'_> {
    fn node(self) -> UiNode {
        let accent = |a: f64| tokens::band_color(self.slot, a);
        let title = TextStyle {
            color: accent(1.0),
            ..ty::CARD_TITLE
        };
        let col_w = (CARD_W - 24.0) / 4.0;
        let toggle_w = (CARD_W - 36.0) * 0.5;
        let knobs = self
            .knobs
            .iter()
            .map(|p| {
                Knob {
                    param: *p,
                    band: Some(self.slot),
                }
                .node()
                .size(col_w, 40.0)
            })
            .collect::<alloc::vec::Vec<_>>();
        let plot = Plot {
            threshold: self.knobs[0],
            ratio: self.knobs[1],
            makeup: self.knobs[3],
            style: Some((self.style, STYLES)),
            knee: Some(self.knobs[2]),
            gr_probe: self.gr_probe,
            band: self.slot,
        }
        .node();
        ui! {
            <Stack w={Dim::Px(CARD_W)} h={Dim::Px(CARD_H)} bg={tokens::BG_2}
                   radius={tokens::RADIUS_MD} border={(1.0, tokens::LINE)} shadow={tokens::ELEV_1}>
                <Shape kind=Rect w={Dim::Px(CARD_W - 12.0)} h=1 bg={accent(0.55)}
                       margin={Edges { l: 6.0, ..Edges::ZERO }}/>
                <Shape kind=RRect w=3 h={Dim::Px(CARD_H - 16.0)} radius=1.5 bg={accent(1.0)}
                       margin={Edges { t: 8.0, ..Edges::ZERO }}/>
                <Column w=Fill h=Fill>
                    <Row h=22 gap=4 margin={Edges { t: 8.0, r: 4.0, b: 0.0, l: 14.0 }}>
                        <Text class={title} w=70 h=22 margin={Edges { t: 2.0, ..Edges::ZERO }}>{self.title}</Text>
                        <Spacer/>
                        <Selector param={self.sat} labels={SATURATIONS} pill={true} w=74 h=22/>
                        <Selector param={self.style} labels={STYLES} pill={true} w=72 h=22/>
                    </Row>
                    {stave_core::ui::grid(4, knobs)
                        .gap_y(34.0)
                        .margin_edges(Edges { t: 10.0, r: 12.0, b: 0.0, l: 12.0 })}
                    <Text class={ty::PLOT_CAPTION} w={Dim::Px(CARD_W - 28.0)} h=12
                          margin={Edges { t: 38.0, l: 14.0, ..Edges::ZERO }}>"TRANSFER"</Text>
                    {plot.size(CARD_W - 28.0, 52.0).margin_edges(Edges { t: 2.0, l: 14.0, ..Edges::ZERO })}
                    <Row gap=12 margin={Edges { t: 8.0, r: 12.0, b: 0.0, l: 12.0 }}>
                        <Toggle param={self.solo} solo={true} band={Some(self.slot)} w={Dim::Px(toggle_w)} h=26/>
                        <Toggle param={self.mute} solo={false} band={Some(self.slot)} w={Dim::Px(toggle_w)} h=26/>
                    </Row>
                </Column>
            </Stack>
        }
    }
}

/// The crossover footer: a captioned panel of split knobs.
fn crossover(params: &[&dyn AnyParam]) -> UiNode {
    let knobs = params
        .iter()
        .map(|p| {
            Knob {
                param: *p,
                band: None,
            }
            .node()
            .size(150.0, 40.0)
        })
        .collect::<alloc::vec::Vec<_>>();
    let w = knobs.len() as f64 * 150.0 + 32.0;
    let caption = TextStyle {
        role: TypeRole::Caption,
        size: 11.0,
        kern: 1.4,
        color: tokens::INK_MUTED,
    };
    ui! {
        <Column w={Dim::Px(w)} h=100 gap=2 bg={tokens::BG_2} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)} pad={Edges { t: 8.0, r: 16.0, b: 0.0, l: 16.0 }}>
            <Text class={caption} w=Fill h=12>"CROSSOVER"</Text>
            {stave_core::ui::row(knobs)}
        </Column>
    }
}
