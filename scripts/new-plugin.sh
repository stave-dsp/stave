#!/usr/bin/env bash
# Stamp a new stave plugin project — the whole canonical shape,
# gate-passing on arrival: declarations, an honest-passthrough
# engine, a house-styled editor, and the full colocated test suite
# (validation / editor bindings / no-alloc / perf / a starter property).
#
# Usage:
#   scripts/new-plugin.sh <name>       # lowercase, hyphens ok: "fuzz", "tape-echo"
#
# Produces plugins/<name>/ (crate stave-<name>-vst3, bundle
# Stave<Name>), registers the workspace member, and prints the next
# steps. CIDs are freshly generated and collision-checked against
# every existing project — ids are forever, so they are minted
# exactly once, here.
#
# The descriptor's vendor fields come from the environment (set them
# for your own plugins; the defaults are the stave author's):
#   STAVE_BUNDLE_VENDOR  reverse-DNS id prefix   (com.ryanbahan.stave)
#   STAVE_VENDOR_NAME    vendor display name     (Ryan Bahan)
#   STAVE_VENDOR_URL     vendor / project URL    (https://github.com/ryancbahan/stave)

set -euo pipefail

NAME="${1:?usage: new-plugin.sh <name> (lowercase, hyphens ok)}"

if [[ ! "$NAME" =~ ^[a-z][a-z0-9-]*$ ]]; then
  echo "error: name must be lowercase alphanumeric with hyphens: '$NAME'" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "${REPO_ROOT}/scripts/lib/bundle-common.sh"
VENDOR_NAME="${STAVE_VENDOR_NAME:-Ryan Bahan}"
VENDOR_URL="${STAVE_VENDOR_URL:-https://github.com/ryancbahan/stave}"
DIR="${REPO_ROOT}/plugins/${NAME}"
CRATE="stave-${NAME}-vst3"
UNDERSCORED="${CRATE//-/_}"

if [[ -d "$DIR" ]]; then
  echo "error: ${DIR} already exists" >&2
  exit 1
fi

# Display name: "tape-echo" -> "Tape Echo"; struct name: "TapeEcho".
DISPLAY=$(echo "$NAME" | awk -F- '{for(i=1;i<=NF;i++){printf "%s%s%s", (i>1?" ":""), toupper(substr($i,1,1)), substr($i,2)}}')
STRUCT="${DISPLAY// /}"
DISPLAY_UPPER=$(echo "$DISPLAY" | tr '[:lower:]' '[:upper:]')

# Mint the two CIDs: four random u32 words each, collision-checked
# against every CID already in the tree (ids are FOREVER).
mint_cid() {
  while :; do
    local words
    words=$(od -An -tx4 -N16 /dev/urandom | tr -s ' ' '\n' | grep -v '^$' \
      | awk '{printf "0x%s%s", toupper($0), (NR<4?", ":"")}' \
      | sed 's/0X/0x/g')
    local first=${words%%,*}
    if ! grep -rq "$first" "${REPO_ROOT}/projects" 2>/dev/null; then
      echo "($words)"
      return
    fi
  done
}
COMPONENT_CID=$(mint_cid)
CONTROLLER_CID=$(mint_cid)

mkdir -p "${DIR}/src" "${DIR}/tests"

# ---------------------------------------------------------------- Cargo.toml
cat > "${DIR}/Cargo.toml" <<EOF
[package]
name = "${CRATE}"
version = "0.0.1"
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
description = "Stave ${DISPLAY} (scaffold: honest passthrough)."

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
stave-core.workspace = true
stave-ui.workspace = true
stave-vst3.workspace = true
stave-clap.workspace = true
stave-dsp.workspace = true

[dev-dependencies]
assert_no_alloc = "1"
stave-test = { path = "../../crates/stave-test" }
stave-host = { path = "../../crates/stave-host" }

[lints]
workspace = true
EOF

# ------------------------------------------------------------------- lib.rs
cat > "${DIR}/src/lib.rs" <<EOF
//! Stave ${DISPLAY}. **Start here**: this file is the plugin's
//! root; everything below is one click away.
//!
//! \`\`\`text
//! lib.rs      ← you are here: identity, ports, params, editor —
//!               the DECLARATIONS half, no behavior
//! params.rs   every knob, with its forever-stable id
//! editor.rs   the UI tree, a pure function of params
//! engine.rs   the ENGINE half — grows into engine/ components the
//!               day the second concept arrives (the growth rule)
//! tests/      the shared checks: validation, editor bindings, no-alloc,
//!               perf, and YOUR PROPERTIES (properties.rs — write the first one
//!               BEFORE the DSP; plugins/tilt tells you why)
//! \`\`\`
//!
//! STATUS: scaffold — an honest passthrough (output trim + bypass).

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{PluginDescriptor, PluginFeature, PortConfig, Spec, StavePlugin};
use stave_vst3::stave_vst3_export;

mod editor;
mod engine;
mod params;

pub use engine::${STRUCT}Engine;
pub use params::${STRUCT}Params;

impl Spec for ${STRUCT}Engine {
    /// Stereo FX, stated — the negotiation surface is a declaration.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: c"${STAVE_BUNDLE_VENDOR}.${NAME}.vst3",
        name: c"Stave ${DISPLAY}",
        vendor: c"${VENDOR_NAME}",
        version: c"0.0.1",
        url: c"${VENDOR_URL}",
        description: c"Stave ${DISPLAY} (scaffold: honest passthrough).",
        features: &[PluginFeature::AudioEffect, PluginFeature::Stereo],
    };

    fn params() -> Arc<${STRUCT}Params> {
        ${STRUCT}Params::new()
    }
    fn editor(params: &${STRUCT}Params) -> Option<stave_core::UiNode> {
        Some(editor::editor(params))
    }
}

/// The plugin: engine + declarations under the framework shell.
pub type ${STRUCT} = StavePlugin<${STRUCT}Engine>;

stave_vst3_export!(
    ${STRUCT}Engine,
    component_cid: ${COMPONENT_CID},
    controller_cid: ${CONTROLLER_CID},
);

stave_clap::stave_clap_export!(${STRUCT}Engine); // CLAP build, same cdylib
EOF

# ---------------------------------------------------------------- params.rs
cat > "${DIR}/src/params.rs" <<EOF
//! ${STRUCT}'s typed parameter table. Ids are FOREVER — pick once,
//! never reuse or renumber.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{BoolParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit};

#[derive(Params)]
pub struct ${STRUCT}Params {
    /// Output trim. Smoothing is DECLARED here (table-owned ramp),
    /// never hand-rolled in the engine.
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
}

impl ${STRUCT}Params {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            output: FloatParam::new(1, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(2, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags { bypass: true, ..Default::default() }),
        })
    }
}
EOF

# ---------------------------------------------------------------- engine.rs
cat > "${DIR}/src/engine.rs" <<EOF
//! The ${NAME} ENGINE — currently an honest passthrough behind the
//! declared output glide. Map every piece you add to its kernel form
//! first (docs/writing-a-plugin.md, "The engine half"), and split
//! into engine/ components the day the second concept arrives.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::ContinuousControl;
use stave_core::{Environment, ProcessCtx, Tail};

use crate::params::${STRUCT}Params;

#[inline]
fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub struct ${STRUCT}Engine {
    params: Arc<${STRUCT}Params>,
    /// The output glide's render state: smoothing is the processor's.
    output: ContinuousControl,
}

impl stave_core::Processor for ${STRUCT}Engine {
    type Params = ${STRUCT}Params;

    fn new(params: Arc<${STRUCT}Params>) -> Self {
        Self {
            output: ContinuousControl::new(&params.output),
            params,
        }
    }

    fn activate(&mut self, env: Environment) {
        self.output
            .prepare(f64::from(env.sample_rate), &self.params.output);
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        let mut out_ramp = self.output.begin_block(ctx.params, &p.output);
        let lanes = ctx.lanes::<2>();
        let active = lanes.active();
        lanes.run(|fr| {
            let g = db_to_amp(out_ramp.next());
            for lane in 0..active.min(2) {
                fr.write(lane, fr.input(lane) * g);
            }
        });
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        Tail::None
    }
}
EOF

# ---------------------------------------------------------------- editor.rs
cat > "${DIR}/src/editor.rs" <<EOF
//! The ${NAME} editor as owned \`ui!\` markup — the whole window, a
//! pure function of params.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::{Knob, Power};
use stave_core::params::AnyParam;
use stave_core::ui::tokens::{self, text as ty};
use stave_core::ui::{ui, Dim, TextStyle, TypeRole};
use stave_core::UiNode;

use crate::params::${STRUCT}Params;

const CELL_W: f64 = 100.0;
const CELL_H: f64 = 92.0;

const CAPTION: TextStyle = TextStyle {
    role: TypeRole::Tick,
    size: 9.0,
    kern: 1.6,
    color: tokens::INK_MUTED,
};

fn card(caption: &'static str, cells: Vec<UiNode>, cols: u16) -> UiNode {
    let w = cols as f64 * CELL_W + 24.0;
    ui! {
        <Column w={Dim::Px(w)} pad=12 gap=6 bg={tokens::BG_1} radius={tokens::RADIUS_MD}
                border={(1.0, tokens::LINE)}>
            <Text class={CAPTION}>{caption}</Text>
            {stave_core::ui::grid(cols, cells)}
        </Column>
    }
}

fn knob(p: &dyn AnyParam) -> UiNode {
    Knob { param: p, band: None }.node().size(CELL_W, CELL_H)
}

pub fn editor(p: &${STRUCT}Params) -> UiNode {
    ui! {
        <Column w=280 bg={tokens::BG_0}>
            <Row w={Dim::Fill(1.0)} h=42
                 pad={stave_core::ui::Edges { t: 9.0, r: 12.0, b: 0.0, l: 14.0 }} gap=8>
                <Text class={ty::WORDMARK} tag="bypass-dim">"STAVE"</Text>
                <Text class={ty::SECTION}>"${DISPLAY_UPPER}"</Text>
                <Spacer/>
                <Power param={&p.bypass} w=24 h=24/>
            </Row>
            <Shape kind=Line w={Dim::Fill(1.0)} h=1 bg={tokens::LINE}/>
            <Row w={Dim::Fill(1.0)} pad=18 gap=16 justify=Center>
                {card("OUTPUT", alloc::vec![knob(&p.output)], 1)}
            </Row>
        </Column>
    }
}
EOF

# ------------------------------------------------------------------- tests/
cat > "${DIR}/tests/validation.rs" <<EOF
//! The validation suite — the price of admission to plugins/.

#[test]
fn passes_validation() {
    stave_test::validation::assert_passes(stave_host::entry_points!(${UNDERSCORED}));
}
EOF

cat > "${DIR}/tests/editor.rs" <<EOF
//! Owned-editor checks: the tree resolves against the table; widgets
//! agree with declared endpoints.

use stave_test::editor::{endpoints_agree, owned_tree_resolves};

#[test]
fn tree_resolves() {
    owned_tree_resolves::<${UNDERSCORED}::${STRUCT}>("${NAME}");
}

#[test]
fn widgets_agree_with_declared_endpoints() {
    endpoints_agree::<${UNDERSCORED}::${STRUCT}>("${NAME}");
}
EOF

cat > "${DIR}/tests/no_alloc.rs" <<EOF
//! The no-alloc check: the full process loop under the guard while
//! params sweep. Grow the wiggle list with every param that
//! rebuilds tables or switches structure — not just the easy trims.
#![cfg(debug_assertions)]

use assert_no_alloc::AllocDisabler;
use stave_test::no_alloc::{check_no_alloc, NoAllocConfig};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_never_allocates() {
    check_no_alloc::<${UNDERSCORED}::${STRUCT}>("${NAME}", &["output"], &NoAllocConfig::default());
}
EOF

cat > "${DIR}/tests/perf.rs" <<EOF
//! The perf report: representative configurations measured against
//! the realtime budget (stave_test::perf — it documents, it
//! does not block). Record the measured baselines HERE when they
//! legitimately move.

use stave_test::perf::{perf_report, PerfCase, PerfConfig};

#[test]
fn perf_report_for_the_record() {
    perf_report::<${UNDERSCORED}::${STRUCT}>(
        "${NAME}",
        &[PerfCase { name: "DEFAULT", set: &[] }],
        &PerfConfig::default(),
    );
}
EOF

cat > "${DIR}/tests/properties.rs" <<EOF
//! ${STRUCT}'s OWN properties — the tests only this project can write.
//! The shared checks prove the plugin is SANE; only a property proves
//! it is RIGHT (plugins/tilt/tests/properties.rs is the cautionary tale:
//! a backwards knob passed every shared check). Write the first
//! property BEFORE the DSP it constrains; measure first, then code to
//! the measurement.
//!
//! The starter property pins the scaffold's one promise — unity
//! passthrough is BIT-exact. Replace it the day the engine grows a
//! voice.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = ${UNDERSCORED}::${STRUCT}::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in set {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = Vec::with_capacity(input.len());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
        in_r.copy_from_slice(&in_l);
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: inb,
            output: outb,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    out
}

/// The scaffold's one promise: at 0 dB trim the passthrough is
/// BIT-exact, sample for sample.
#[test]
fn unity_passthrough_is_bit_exact() {
    let input: Vec<f32> = (0..12_800)
        .map(|i| (2.0 * std::f32::consts::PI * 997.0 * i as f32 / FS).sin() * 0.5)
        .collect();
    let out = render(&[("output", 0.0)], &input);
    for (i, (x, y)) in input.iter().zip(&out).enumerate() {
        assert!(x.to_bits() == y.to_bits(), "sample {i}: {x} vs {y}");
    }
}
EOF

# --------------------------------------------------- workspace registration
python3 - "$REPO_ROOT" "$NAME" <<'PYEOF'
import sys
root, name = sys.argv[1], sys.argv[2]
p = f"{root}/Cargo.toml"
s = open(p).read()
member = f'    "plugins/{name}",\n'
if member in s:
    print("workspace: already registered")
else:
    i = s.index("members = [")
    j = s.index("]", i)
    s = s[:j] + member + s[j:]
    open(p, "w").write(s)
    print(f"workspace: registered plugins/{name}")
PYEOF

echo ""
echo "stamped: plugins/${NAME}  (crate ${CRATE}, bundle Stave${STRUCT}.vst3)"
echo ""
echo "next steps:"
echo "  cargo test -p ${CRATE}                     # the shared checks, green on arrival"
echo "  bash scripts/install.sh ${CRATE} release   # build + bundle + validate + install"
echo "  ./target/release/stave-bench build/Stave${STRUCT}.vst3 --describe"
echo "  ./target/release/stave-play  build/Stave${STRUCT}.vst3 --screenshot /tmp/${NAME}.png"
echo ""
echo "then read docs/writing-a-plugin.md with plugins/tilt open beside it,"
echo "and write your first property in tests/properties.rs before the DSP it constrains."
