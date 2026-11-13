//! A `.nam` WaveNet config → layer-array shapes: legacy keys and the
//! 0.13 trainer's, with every feature this reader does not implement
//! refused by name; a slimmable WaveNet → one network per width.

use super::{mono, size};
use crate::learned::activation::Activation;
use crate::learned::error::LoadError;
use crate::learned::json::Json;
use crate::learned::wavenet::{ArraySpec, Gating, LayerSpec, WaveNet};

/// FiLM (feature-wise modulation) sites a 0.13 layer array may switch on.
const FILM: [&str; 8] = [
    "conv_pre_film",
    "conv_post_film",
    "input_mixin_pre_film",
    "input_mixin_post_film",
    "activation_pre_film",
    "activation_post_film",
    "layer1x1_post_film",
    "head1x1_post_film",
];

fn config(what: String) -> LoadError {
    LoadError::Config(what)
}

fn unsupported(what: String) -> LoadError {
    LoadError::Unsupported(what)
}

/// The networks a WaveNet config describes: one, `(None, net)`, or for a
/// slimmable WaveNet one per width in slider order, `(Some(channels),
/// net)`, the last being what the trainer runs unslimmed.
pub(super) fn build(
    config_json: &Json,
    weights: &[f32],
    sample_rate: f32,
) -> Result<Vec<(Option<usize>, WaveNet)>, LoadError> {
    if config_json.field("condition_dsp").is_some() {
        return Err(unsupported("WaveNet condition_dsp".into()));
    }
    if config_json.field("head").is_some() {
        return Err(unsupported("WaveNet post-head (\"head\" not null)".into()));
    }
    mono(config_json, "WaveNet")?;
    let layers = config_json
        .field("layers")
        .and_then(Json::as_array)
        .ok_or_else(|| config("WaveNet config needs a \"layers\" list".into()))?;
    let (specs, slims): (Vec<_>, Vec<_>) = layers
        .iter()
        .enumerate()
        .map(|(i, lc)| array(i, lc))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .unzip();
    let full = WaveNet::build(&specs, weights, sample_rate)?;
    let Some(allowed) = slims.into_iter().flatten().next() else {
        return Ok(vec![(None, full)]);
    };
    // NAM 0.13 slims one layer array only (its trainer refuses more).
    if specs.len() != 1 {
        return Err(unsupported(
            "a slimmable WaveNet with more than one layer array".into(),
        ));
    }
    let spec = &specs[0];
    let widest = spec.channels.min(spec.bottleneck);
    if allowed.iter().any(|&c| c == 0 || c > widest) {
        return Err(config(format!(
            "slimmable allowed_channels must be 1 to {widest} (the channels and bottleneck)"
        )));
    }
    allowed
        .into_iter()
        .map(|c| {
            let (slim, w) = slice(spec, weights, c);
            Ok((Some(c), WaveNet::build(&[slim], &w, sample_rate)?))
        })
        .collect()
}

/// Channel slicing (Atkinson 2025; NAM's `slice_channels_uniform`): the
/// network at width `c` is the full one's leading `c` channels — every
/// convolution keeps its first `c` (gated: `2c`) output rows and first
/// `c` input columns, except the array's input (one channel, kept) and
/// the head's output (kept). Residual and bottleneck widths both become
/// `c`. `weights` has already built the full network, so it is the
/// length the spec reads.
fn slice(spec: &ArraySpec, weights: &[f32], c: usize) -> (ArraySpec, Vec<f32>) {
    let mut at = 0;
    let mut out = Vec::new();
    // A `(rows, cols, taps)` tensor and optional bias, keeping the
    // leading `keep_rows` × `keep_cols`.
    let mut take = |rows: usize, cols: usize, taps: usize, bias: bool, keep: (usize, usize)| {
        let (keep_rows, keep_cols) = keep;
        for r in 0..keep_rows {
            for j in 0..keep_cols {
                let from = at + (r * cols + j) * taps;
                out.extend_from_slice(&weights[from..from + taps]);
            }
        }
        at += rows * cols * taps;
        if bias {
            out.extend_from_slice(&weights[at..at + keep_rows]);
            at += rows;
        }
    };
    let (ch, b) = (spec.channels, spec.bottleneck);
    take(ch, spec.input_size, 1, false, (c, spec.input_size));
    for l in &spec.layers {
        let (mid, keep_mid) = match l.gating {
            Gating::None => (b, c),
            _ => (2 * b, 2 * c),
        };
        take(mid, ch, l.kernel, true, (keep_mid, c));
        take(
            mid,
            spec.condition_size,
            1,
            false,
            (keep_mid, spec.condition_size),
        );
        if spec.layer1x1 {
            take(ch, b, 1, true, (c, c));
        }
    }
    let head = spec.head_size;
    take(head, b, spec.head_kernel, spec.head_bias, (head, c));
    out.push(weights[at]);
    let slim = ArraySpec {
        channels: c,
        bottleneck: c,
        layers: spec
            .layers
            .iter()
            .map(|l| LayerSpec {
                activation: l.activation.sliced(c),
                gating: match &l.gating {
                    Gating::None => Gating::None,
                    Gating::Gated(g) => Gating::Gated(g.sliced(c)),
                    Gating::Blended(g) => Gating::Blended(g.sliced(c)),
                },
                ..l.clone()
            })
            .collect(),
        ..spec.clone()
    };
    (slim, out)
}

/// `true` when an optional `{"active": …}` block is switched on
/// (a block without the key counts as on, as NeuralAmpModelerCore reads it).
fn active(block: &Json) -> bool {
    block
        .field("active")
        .and_then(Json::as_bool)
        .unwrap_or(true)
}

fn groups_one(block: &Json, key: &str, i: usize) -> Result<(), LoadError> {
    if block.field(key).is_some() && size(block, key)? != 1 {
        return Err(unsupported(format!(
            "layer array {i}: {key} other than 1 (grouped convolution)"
        )));
    }
    Ok(())
}

/// NAM's one slimming method, and the keys its trainer validates.
const SLIM_METHOD: &str = "slice_channels_uniform";

/// A layer array's `slimmable` block: the widths it may run at, in
/// slider order. `boosting` and `init_strategy` shape training only (a
/// gradient mask and an initialisation): read, checked, no effect on
/// inference.
fn slimmable(i: usize, lc: &Json, channels: usize) -> Result<Option<Vec<usize>>, LoadError> {
    let Some(block) = lc.field("slimmable") else {
        return Ok(None);
    };
    let Json::Object(members) = block else {
        return Err(config(format!(
            "layer array {i}: slimmable must be an object"
        )));
    };
    for (key, _) in members {
        if key != "method" && key != "kwargs" {
            return Err(unsupported(format!(
                "layer array {i}: slimmable field \"{key}\""
            )));
        }
    }
    match block.field("method").and_then(Json::as_str) {
        Some(SLIM_METHOD) => {}
        Some(other) => {
            return Err(unsupported(format!(
                "layer array {i}: slimmable method \"{other}\""
            )))
        }
        None => return Err(config(format!("layer array {i}: slimmable needs a method"))),
    }
    let kwargs = match block.field("kwargs") {
        Some(k @ Json::Object(members)) => {
            for (key, _) in members {
                if !matches!(
                    key.as_str(),
                    "allowed_channels" | "boosting" | "init_strategy"
                ) {
                    return Err(unsupported(format!(
                        "layer array {i}: slimmable kwargs field \"{key}\""
                    )));
                }
            }
            k
        }
        _ => {
            return Err(config(format!(
                "layer array {i}: slimmable needs a kwargs object"
            )))
        }
    };
    if kwargs
        .field("boosting")
        .is_some_and(|b| b.as_bool().is_none())
    {
        return Err(config(format!(
            "layer array {i}: slimmable boosting must be true or false"
        )));
    }
    if kwargs
        .field("init_strategy")
        .is_some_and(|s| s.as_str().is_none())
    {
        return Err(config(format!(
            "layer array {i}: slimmable init_strategy must be a name"
        )));
    }
    let allowed = match kwargs.field("allowed_channels") {
        None => (1..=channels).collect(),
        Some(list) => list
            .as_array()
            .filter(|l| !l.is_empty())
            .and_then(|l| l.iter().map(whole).collect::<Option<Vec<_>>>())
            .ok_or_else(|| {
                config(format!(
                    "layer array {i}: slimmable allowed_channels must be a list of whole numbers"
                ))
            })?,
    };
    let mut seen = allowed.clone();
    seen.sort_unstable();
    seen.dedup();
    if seen.len() != allowed.len() {
        return Err(config(format!(
            "layer array {i}: slimmable allowed_channels repeats a width"
        )));
    }
    Ok(Some(allowed))
}

fn array(i: usize, lc: &Json) -> Result<(ArraySpec, Option<Vec<usize>>), LoadError> {
    for key in FILM {
        if lc.field(key).is_some_and(active) {
            return Err(unsupported(format!("layer array {i}: FiLM ({key})")));
        }
    }
    if lc.field("film_params").is_some() {
        return Err(unsupported(format!("layer array {i}: FiLM (film_params)")));
    }
    groups_one(lc, "groups_input", i)?;
    groups_one(lc, "groups_input_mixin", i)?;

    let channels = size(lc, "channels")?;
    let bottleneck = match lc.field("bottleneck") {
        Some(_) => size(lc, "bottleneck")?,
        None => channels,
    };

    let (head_size, head_kernel, head_dilation, head_bias) = match lc.field("head") {
        Some(h) => (
            size(h, "out_channels")?,
            size(h, "kernel_size")?,
            if h.field("head_dilation").is_some() {
                size(h, "head_dilation")?
            } else {
                1
            },
            h.field("bias").and_then(Json::as_bool).ok_or_else(|| {
                config(format!("layer array {i}: head.bias must be true or false"))
            })?,
        ),
        None => (
            size(lc, "head_size")?,
            1,
            1,
            lc.field("head_bias")
                .and_then(Json::as_bool)
                .ok_or_else(|| {
                    config(format!("layer array {i}: head_bias must be true or false"))
                })?,
        ),
    };

    let layer1x1 = match lc.field("layer1x1") {
        Some(b) if active(b) => {
            groups_one(b, "groups", i)?;
            true
        }
        Some(_) => false,
        None => true,
    };
    let head1x1 = match lc.field("head1x1") {
        Some(b) if active(b) => {
            groups_one(b, "groups", i)?;
            Some(size(b, "out_channels")?)
        }
        _ => None,
    };

    let dilations = lc
        .field("dilations")
        .and_then(Json::as_array)
        .ok_or_else(|| config(format!("layer array {i}: missing \"dilations\"")))?;
    let n = dilations.len();
    let dilations = dilations
        .iter()
        .map(|d| {
            whole(d)
                .ok_or_else(|| config(format!("layer array {i}: dilations must be whole numbers")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let kernels = kernels(i, lc, n)?;
    let activations = per_layer(i, lc.field("activation"), n, "activation")?
        .into_iter()
        .map(|a| {
            a.ok_or_else(|| config(format!("layer array {i}: missing activation")))
                .and_then(activation)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let gatings = gatings(i, lc, n)?;

    let slim = slimmable(i, lc, channels)?;
    if slim.is_some() {
        if head1x1.is_some() {
            return Err(unsupported(format!(
                "layer array {i}: slimmable with an active head 1x1"
            )));
        }
        if head_kernel != 1 {
            return Err(unsupported(format!(
                "layer array {i}: slimmable with a head kernel other than 1"
            )));
        }
    }

    let layers = (0..n)
        .map(|j| LayerSpec {
            kernel: kernels[j],
            dilation: dilations[j],
            activation: activations[j].clone(),
            gating: gatings[j].clone(),
        })
        .collect();
    let spec = ArraySpec {
        input_size: size(lc, "input_size")?,
        condition_size: size(lc, "condition_size")?,
        channels,
        bottleneck,
        layers,
        layer1x1,
        head1x1,
        head_size,
        head_kernel,
        head_dilation,
        head_bias,
    };
    Ok((spec, slim))
}

fn whole(v: &Json) -> Option<usize> {
    v.as_f64()
        .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n < 1e9)
        .map(|n| n as usize)
}

/// `kernel_sizes` (a list) or legacy `kernel_size` (one number, or a list).
fn kernels(i: usize, lc: &Json, n: usize) -> Result<Vec<usize>, LoadError> {
    let v = match (lc.field("kernel_sizes"), lc.field("kernel_size")) {
        (Some(_), Some(_)) => {
            return Err(config(format!(
                "layer array {i}: both kernel_size and kernel_sizes"
            )));
        }
        (Some(v), None) | (None, Some(v)) => v,
        (None, None) => return Err(config(format!("layer array {i}: missing kernel_size"))),
    };
    let bad = || {
        config(format!(
            "layer array {i}: kernel sizes must be whole numbers, one per layer"
        ))
    };
    match v {
        Json::Array(items) if items.len() == n => {
            items.iter().map(|k| whole(k).ok_or_else(bad)).collect()
        }
        Json::Array(_) => Err(bad()),
        k => whole(k).map(|k| vec![k; n]).ok_or_else(bad),
    }
}

/// One value for every layer, or a list of one per layer (`null`
/// entries kept as `None`).
fn per_layer<'a>(
    i: usize,
    v: Option<&'a Json>,
    n: usize,
    key: &str,
) -> Result<Vec<Option<&'a Json>>, LoadError> {
    match v {
        None => Ok(vec![None; n]),
        Some(Json::Array(items)) if items.len() == n => Ok(items
            .iter()
            .map(|a| (!matches!(a, Json::Null)).then_some(a))
            .collect()),
        Some(Json::Array(_)) => Err(config(format!(
            "layer array {i}: {key} list needs one entry per layer"
        ))),
        Some(one) => Ok(vec![Some(one); n]),
    }
}

/// `gating_mode` (+ `secondary_activation`, Sigmoid when absent), or
/// legacy `gated` (Sigmoid gate), or none.
fn gatings(i: usize, lc: &Json, n: usize) -> Result<Vec<Gating>, LoadError> {
    if let Some(modes) = lc.field("gating_mode") {
        let modes = per_layer(i, Some(modes), n, "gating_mode")?;
        let seconds = per_layer(
            i,
            lc.field("secondary_activation"),
            n,
            "secondary_activation",
        )?;
        return modes
            .into_iter()
            .zip(seconds)
            .map(|(mode, second)| {
                let gate = || second.map_or(Ok(Activation::Sigmoid), activation);
                match mode.and_then(Json::as_str) {
                    None | Some("none") => Ok(Gating::None),
                    Some("gated") => Ok(Gating::Gated(gate()?)),
                    Some("blended") => Ok(Gating::Blended(gate()?)),
                    Some(other) => Err(unsupported(format!(
                        "layer array {i}: gating_mode \"{other}\""
                    ))),
                }
            })
            .collect();
    }
    let gated = match lc.field("gated") {
        None => false,
        Some(g) => g
            .as_bool()
            .ok_or_else(|| config(format!("layer array {i}: gated must be true or false")))?,
    };
    Ok(vec![
        if gated {
            Gating::Gated(Activation::Sigmoid)
        } else {
            Gating::None
        };
        n
    ])
}

/// An activation by name (`"Tanh"`) or object (`{"type": "LeakyReLU",
/// "negative_slope": 0.1}`).
fn activation(v: &Json) -> Result<Activation, LoadError> {
    let (name, params) = match v {
        Json::String(s) => (s.as_str(), None),
        Json::Object(_) => (
            v.field("type")
                .and_then(Json::as_str)
                .ok_or_else(|| config("activation object without a \"type\"".into()))?,
            Some(v),
        ),
        _ => return Err(config("activation must be a name or an object".into())),
    };
    let num = |key: &str, default: f32| {
        params
            .and_then(|p| p.field(key))
            .and_then(Json::as_f64)
            .map_or(default, |x| x as f32)
    };
    Ok(match name {
        "Tanh" => Activation::Tanh,
        "Hardtanh" => Activation::Hardtanh {
            min: num("min_val", -1.0),
            max: num("max_val", 1.0),
        },
        "ReLU" => Activation::Relu,
        "LeakyReLU" => Activation::LeakyRelu {
            slope: num("negative_slope", 0.01),
        },
        "PReLU" => {
            let p = params.ok_or_else(|| config("PReLU needs its slope".into()))?;
            let slopes = if let Some(list) = p.field("negative_slopes").and_then(Json::as_array) {
                list.iter()
                    .map(|s| s.as_f64().map(|x| x as f32))
                    .collect::<Option<Vec<_>>>()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| config("PReLU negative_slopes must be numbers".into()))?
            } else {
                vec![p
                    .field("negative_slope")
                    .and_then(Json::as_f64)
                    .ok_or_else(|| config("PReLU needs negative_slope or negative_slopes".into()))?
                    as f32]
            };
            Activation::Prelu { slopes }
        }
        "Sigmoid" => Activation::Sigmoid,
        "SiLU" => Activation::Silu,
        "Hardswish" => Activation::Hardswish,
        "Softsign" => Activation::Softsign,
        "LeakyHardtanh" | "LeakyHardTanh" => Activation::LeakyHardtanh {
            min: num("min_val", -1.0),
            max: num("max_val", 1.0),
            min_slope: num("min_slope", 0.01),
            max_slope: num("max_slope", 0.01),
        },
        "Fasttanh" => {
            return Err(unsupported(
                "Fasttanh (NeuralAmpModelerCore's approximation, not a trained activation)".into(),
            ));
        }
        other => return Err(unsupported(format!("activation \"{other}\""))),
    })
}
