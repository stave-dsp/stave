//! DSP.md M2 — the learned networks against their references.
//!
//! 1. **The trainer's own output.** `tests/reference/nam/*.nam` were
//!    exported by Neural Amp Modeler 0.13's exporter, and the paired
//!    `.txt` hold `input output` from its PyTorch forward pass on the
//!    reloaded file (`scripts/nam-reference.py` regenerates them;
//!    `gru.*` is `torch.nn.GRU` itself). Budget: max abs error ≤ 1e-5,
//!    f32 against f32 with different summation orders.
//! 2. **An independent f64 reference.** Random WaveNet shapes over every
//!    switch the reader implements, and random LSTMs and GRUs, written
//!    as `.nam` / JSON text and run through the public loader, against
//!    a whole-sequence f64 forward pass written here from PyTorch's
//!    definitions (valid convolutions over the zero-padded input, as the
//!    trainer computes them; no streaming, no settling). Same budget.

use stave_dsp::learned::{nam, Gru, LoadError};
use stave_dsp::Stage;

const BUDGET: f32 = 1e-5;
const RATE: f32 = 48_000.0;

fn pairs(text: &str) -> (Vec<f32>, Vec<f32>) {
    text.lines()
        .map(|l| {
            let mut it = l.split_whitespace().map(|v| v.parse::<f32>().unwrap());
            (it.next().unwrap(), it.next().unwrap())
        })
        .unzip()
}

/// Run `stage` over `input` sample by sample and as one block; the two
/// must agree bit for bit. Returns the output.
fn run(stage: &mut (impl Stage + Clone), input: &[f32]) -> Vec<f32> {
    let mut block_stage = stage.clone();
    let ticked: Vec<f32> = input.iter().map(|&x| stage.tick(x)).collect();
    let mut block = input.to_vec();
    block_stage.process(&mut block);
    assert_eq!(ticked, block, "process must equal the tick loop");
    ticked
}

fn max_error(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

fn fixture(name: &str, nam_text: &str, expected: &str) -> f32 {
    let nam = nam::parse(nam_text).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(nam.sample_rate, Some(RATE));
    let mut net = nam.network;
    net.prepare(RATE);
    assert!(net.rate_mismatch().is_none());
    let (x, y) = pairs(expected);
    let err = max_error(&run(&mut net, &x), &y);
    println!("{name}: max abs error {err:.2e} over {} samples", x.len());
    assert!(
        err <= BUDGET,
        "{name}: max abs error {err:e} over the {BUDGET:e} budget"
    );
    err
}

macro_rules! reference {
    ($name:literal) => {
        (
            include_str!(concat!("reference/nam/", $name, ".nam")),
            include_str!(concat!("reference/nam/", $name, ".txt")),
        )
    };
}

#[test]
#[cfg_attr(miri, ignore)]
fn wavenet_standard_matches_the_trainer() {
    let (n, t) = reference!("wavenet_standard");
    fixture("wavenet_standard", n, t);
}

#[test]
#[cfg_attr(miri, ignore)]
fn wavenet_packed_matches_the_trainer() {
    let (n, t) = reference!("wavenet_packed");
    fixture("wavenet_packed", n, t);
}

#[test]
#[cfg_attr(miri, ignore)]
fn wavenet_mixed_matches_the_trainer() {
    let (n, t) = reference!("wavenet_mixed");
    fixture("wavenet_mixed", n, t);
}

#[test]
#[cfg_attr(miri, ignore)]
fn lstm_matches_the_trainer() {
    let (n, t) = reference!("lstm");
    fixture("lstm", n, t);
}

#[test]
#[cfg_attr(miri, ignore)]
fn gru_matches_torch() {
    let mut net = Gru::parse(include_str!("reference/nam/gru.json")).unwrap();
    net.prepare(RATE);
    let (x, y) = pairs(include_str!("reference/nam/gru.txt"));
    let err = max_error(&run(&mut net, &x), &y);
    println!("gru: max abs error {err:.2e}");
    assert!(err <= BUDGET, "gru: {err:e}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn legacy_keys_read_the_same_network() {
    // The standard fixture's network restated in the pre-0.13 keys
    // (head_size / head_bias / kernel_size / gated) must be the same
    // network bit for bit.
    let (text, expected) = reference!("wavenet_standard");
    let start = text.find("\"weights\":").unwrap();
    let end = start + text[start..].find(']').unwrap() + 1;
    let legacy = format!(
        r#"{{"version": "0.5.2", "architecture": "WaveNet", "sample_rate": 48000,
        "config": {{"layers": [
          {{"input_size": 1, "condition_size": 1, "channels": 4, "head_size": 3, "head_bias": false,
            "kernel_size": 3, "dilations": [1, 2, 4], "activation": "Tanh", "gated": false}},
          {{"input_size": 4, "condition_size": 1, "channels": 3, "head_size": 1, "head_bias": true,
            "kernel_size": 3, "dilations": [1, 2], "activation": "Tanh", "gated": true}}],
          "head": null, "head_scale": 0.3}},
        {}}}"#,
        &text[start..end]
    );
    let (x, _) = pairs(expected);
    let mut new = nam::parse(text).unwrap().network;
    let mut old = nam::parse(&legacy).unwrap().network;
    assert_eq!(run(&mut new, &x), run(&mut old, &x));
}

#[test]
fn errors_name_the_problem() {
    let (text, _) = reference!("lstm");
    let short = text.replacen("[-0.4225,", "[", 1);
    assert_eq!(
        nam::parse(&short).unwrap_err(),
        LoadError::Weights {
            expected: 386,
            found: 385
        }
    );
    assert!(nam::parse(&text.replace("\"LSTM\"", "\"Linear\""))
        .unwrap_err()
        .to_string()
        .contains("Linear"));
}

/// Run one size of a file against a trainer reference.
fn size_matches(name: &str, net: nam::Network, expected: &str) -> f32 {
    let mut net = net;
    net.prepare(RATE);
    let (x, y) = pairs(expected);
    let err = max_error(&run(&mut net, &x), &y);
    println!(
        "{name}: max abs error {err:.2e}, {} multiply-adds",
        net.multiply_adds()
    );
    assert!(
        err <= BUDGET,
        "{name}: max abs error {err:e} over the {BUDGET:e} budget"
    );
    err
}

/// DSP.md M7: a WaveNet trained for channel slicing (Atkinson 2025),
/// exported by NAM 0.13, at every width it offers against NAM's forward
/// pass slimmed to that width; the default network is the widest.
#[test]
#[cfg_attr(miri, ignore)]
fn slimmable_widths_match_the_trainer() {
    type Widths = &'static [(usize, &'static str)];
    let files: [(&str, &str, Widths); 2] = [
        (
            "slimmable_gated",
            include_str!("reference/nam/slimmable_gated.nam"),
            &[
                (1, include_str!("reference/nam/slimmable_gated_c1.txt")),
                (2, include_str!("reference/nam/slimmable_gated_c2.txt")),
                (3, include_str!("reference/nam/slimmable_gated_c3.txt")),
                (4, include_str!("reference/nam/slimmable_gated_c4.txt")),
            ],
        ),
        (
            "slimmable_plain",
            include_str!("reference/nam/slimmable_plain.nam"),
            &[
                (2, include_str!("reference/nam/slimmable_plain_c2.txt")),
                (4, include_str!("reference/nam/slimmable_plain_c4.txt")),
                (6, include_str!("reference/nam/slimmable_plain_c6.txt")),
            ],
        ),
    ];
    for (name, text, widths) in files {
        let file = nam::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let sizes = file.sizes();
        assert_eq!(sizes.len(), widths.len(), "{name}");
        for (i, (&(channels, expected), size)) in widths.iter().zip(sizes).enumerate() {
            assert_eq!(size.channels, Some(channels), "{name}");
            assert_eq!(size.ratio, i as f32 / widths.len() as f32, "{name}");
            let net = file.network_at(i);
            assert_eq!(net.multiply_adds(), size.multiply_adds);
            size_matches(&format!("{name} at {channels} channels"), net, expected);
            // NAM's slider in the middle of the size's interval selects it.
            let mid = (i as f32 + 0.5) / widths.len() as f32;
            assert_eq!(file.slimmed(mid).multiply_adds(), size.multiply_adds);
        }
        assert!(sizes
            .windows(2)
            .all(|w| w[0].multiply_adds < w[1].multiply_adds));
        let (_, widest) = widths[widths.len() - 1];
        size_matches(&format!("{name} default"), file.network, widest);
    }
}

/// DSP.md M7: a `SlimmableContainer` of three NAM-exported submodels
/// (two WaveNets and an LSTM): each submodel against the trainer's
/// forward pass of it, offered smallest first, the default the largest.
#[test]
#[cfg_attr(miri, ignore)]
fn container_submodels_match_the_trainer() {
    let text = include_str!("reference/nam/container.nam");
    let expected = [
        include_str!("reference/nam/container_0.txt"),
        include_str!("reference/nam/container_1.txt"),
        include_str!("reference/nam/container_2.txt"),
    ];
    let file = nam::parse(text).unwrap();
    assert_eq!(file.sample_rate, Some(RATE));
    let sizes = file.sizes();
    assert_eq!(sizes.len(), 3);
    assert!(sizes
        .windows(2)
        .all(|w| w[0].multiply_adds <= w[1].multiply_adds));
    // The file lists its submodels smallest first, so slider order is file order.
    for (i, reference) in expected.iter().enumerate() {
        assert_eq!(sizes[i].channels, None);
        size_matches(
            &format!("container submodel {i}"),
            file.network_at(i),
            reference,
        );
    }
    assert!(matches!(file.network_at(2), nam::Network::Lstm(_)));
    size_matches("container default", file.network, expected[2]);

    // Listed largest first, the sizes are the same, still smallest first.
    let reversed = reverse_submodels(text);
    let file = nam::parse(&reversed).unwrap();
    for (i, reference) in expected.iter().enumerate() {
        size_matches(
            &format!("reversed submodel {i}"),
            file.network_at(i),
            reference,
        );
    }
}

/// The container's submodel list, reversed (each entry is one
/// `{"model":…}` object; the split points are where one ends and the
/// next begins).
fn reverse_submodels(text: &str) -> String {
    let start = text.find("\"submodels\":[").unwrap() + "\"submodels\":[".len();
    let mut depth = 0;
    let mut entries = Vec::new();
    let mut from = start;
    let mut end = start;
    for (i, c) in text[start..].char_indices() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' if depth == 0 => {
                end = start + i;
                break;
            }
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    entries.push(&text[from..start + i + 1]);
                }
            }
            ',' if depth == 0 => from = start + i + 1,
            _ => {}
        }
    }
    assert_eq!(entries.len(), 3);
    entries.reverse();
    format!("{}{}{}", &text[..start], entries.join(","), &text[end..])
}

/// Slimmable and container fields this reader does not implement are
/// refused by name, never ignored.
#[test]
#[cfg_attr(miri, ignore)]
fn slimmable_fields_are_refused_by_name() {
    let refused = |text: String| match nam::parse(&text) {
        Err(LoadError::Unsupported(what)) => what,
        other => panic!("expected Unsupported, got {other:?}"),
    };
    let slim = include_str!("reference/nam/slimmable_plain.nam");
    assert!(refused(slim.replace("\"slice_channels_uniform\"", "\"prune\"")).contains("prune"));
    assert!(refused(slim.replace(
        "\"allowed_channels\"",
        "\"min_channels\":1,\"allowed_channels\""
    ))
    .contains("min_channels"));
    let container = include_str!("reference/nam/container.nam");
    assert!(
        refused(container.replacen("{\"model\":", "{\"max_value\":0.5,\"model\":", 1))
            .contains("max_value")
    );
    assert!(
        refused(container.replace("\"submodels\":", "\"selector\":\"x\",\"submodels\":"))
            .contains("selector")
    );
    // A container inside a container, and a slimmable WaveNet inside one.
    let nested = container.replacen(
        "{\"model\":",
        &format!("{{\"model\":{}}},{{\"model\":", container.trim()),
        1,
    );
    assert!(refused(nested).contains("inside a SlimmableContainer"));
    let inner = container.replacen(
        "{\"model\":",
        &format!("{{\"model\":{}}},{{\"model\":", slim.trim()),
        1,
    );
    assert!(refused(inner).contains("inside a container"));
    // Widths past the channels are a malformed file.
    assert!(matches!(
        nam::parse(&slim.replace("[2,4,6]", "[2,4,8]")),
        Err(LoadError::Config(_))
    ));
}

// ----- the independent f64 reference -----

/// A fixed LCG: every random shape and weight is reproducible.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() * n as f64) as usize % n
    }
    /// A weight in ±`scale`, rounded to four decimals so the text and
    /// the f64 reference hold the same value.
    fn weight(&mut self, scale: f64) -> f64 {
        let w = (self.next() * 2.0 - 1.0) * scale;
        (w * 1e4).round() / 1e4
    }
}

#[derive(Clone, Debug)]
enum Act {
    Tanh,
    Sigmoid,
    Relu,
    Leaky(f64),
    Hardtanh(f64, f64),
    Silu,
    Softsign,
    Hardswish,
    Prelu(Vec<f64>),
}

impl Act {
    fn random(r: &mut Lcg, channels: usize) -> Act {
        match r.below(9) {
            0 => Act::Tanh,
            1 => Act::Sigmoid,
            2 => Act::Relu,
            3 => Act::Leaky(0.05),
            4 => Act::Hardtanh(-0.6, 0.8),
            5 => Act::Silu,
            6 => Act::Softsign,
            7 => Act::Hardswish,
            _ => Act::Prelu((0..channels).map(|_| r.weight(0.5)).collect()),
        }
    }
    fn json(&self) -> String {
        match self {
            Act::Tanh => r#"{"type":"Tanh"}"#.into(),
            Act::Sigmoid => r#"{"type":"Sigmoid"}"#.into(),
            Act::Relu => r#"{"type":"ReLU"}"#.into(),
            Act::Leaky(s) => format!(r#"{{"type":"LeakyReLU","negative_slope":{s}}}"#),
            Act::Hardtanh(a, b) => format!(r#"{{"type":"Hardtanh","min_val":{a},"max_val":{b}}}"#),
            Act::Silu => r#"{"type":"SiLU"}"#.into(),
            Act::Softsign => r#"{"type":"Softsign"}"#.into(),
            Act::Hardswish => r#"{"type":"Hardswish"}"#.into(),
            Act::Prelu(s) => format!(r#"{{"type":"PReLU","negative_slopes":{s:?}}}"#),
        }
    }
    fn f(&self, x: f64, ch: usize) -> f64 {
        let sig = |x: f64| 1.0 / (1.0 + (-x).exp());
        match self {
            Act::Tanh => x.tanh(),
            Act::Sigmoid => sig(x),
            Act::Relu => x.max(0.0),
            Act::Leaky(s) => {
                if x >= 0.0 {
                    x
                } else {
                    s * x
                }
            }
            Act::Hardtanh(a, b) => x.clamp(*a, *b),
            Act::Silu => x * sig(x),
            Act::Softsign => x / (1.0 + x.abs()),
            Act::Hardswish => x * (x + 3.0).clamp(0.0, 6.0) / 6.0,
            Act::Prelu(s) => {
                if x >= 0.0 {
                    x
                } else {
                    s[ch] * x
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Gate {
    None,
    Gated(Act),
    Blended(Act),
}

#[derive(Clone, Debug)]
struct Layer {
    k: usize,
    d: usize,
    act: Act,
    gate: Gate,
}

#[derive(Clone, Debug)]
struct Array {
    input: usize,
    channels: usize,
    bottleneck: usize,
    layers: Vec<Layer>,
    layer1x1: bool,
    head1x1: Option<usize>,
    head_size: usize,
    head_k: usize,
    head_bias: bool,
}

impl Array {
    fn head_width(&self) -> usize {
        self.head1x1.unwrap_or(self.bottleneck)
    }
}

/// A random chain of layer arrays that fit together.
fn random_wavenet(r: &mut Lcg) -> Vec<Array> {
    let n = 1 + r.below(3);
    let mut arrays: Vec<Array> = Vec::new();
    for i in 0..n {
        let channels = 1 + r.below(5);
        let layer1x1 = r.below(4) != 0;
        let bottleneck = if layer1x1 {
            1 + r.below(channels)
        } else {
            channels
        };
        let head1x1 = (r.below(3) == 0).then(|| 1 + r.below(3));
        let layers = (0..1 + r.below(4))
            .map(|_| {
                let gate = match r.below(3) {
                    0 => Gate::None,
                    1 => Gate::Gated(Act::random(r, bottleneck)),
                    _ => Gate::Blended(Act::random(r, bottleneck)),
                };
                Layer {
                    k: 1 + r.below(4),
                    d: 1 + r.below(5),
                    act: Act::random(r, bottleneck),
                    gate,
                }
            })
            .collect();
        let input = if i == 0 { 1 } else { arrays[i - 1].channels };
        let a = Array {
            input,
            channels,
            bottleneck,
            layers,
            layer1x1,
            head1x1,
            head_size: 1,
            head_k: 1 + r.below(3),
            head_bias: r.below(2) == 0,
        };
        if i > 0 {
            arrays[i - 1].head_size = a.head_width();
        }
        arrays.push(a);
    }
    arrays
}

/// PyTorch `Conv1d` weights and bias, read in the exporter's order.
struct Conv {
    w: Vec<f64>, // (out, in, k) row-major
    b: Vec<f64>,
    inp: usize,
    out: usize,
    k: usize,
    d: usize,
}

impl Conv {
    fn read(
        ws: &mut std::slice::Iter<'_, f64>,
        inp: usize,
        out: usize,
        k: usize,
        d: usize,
        bias: bool,
    ) -> Conv {
        let w = ws.by_ref().take(out * inp * k).copied().collect();
        let b = if bias {
            ws.by_ref().take(out).copied().collect()
        } else {
            vec![0.0; out]
        };
        Conv {
            w,
            b,
            inp,
            out,
            k,
            d,
        }
    }
    /// Valid convolution: output length `len − (k−1)·d`.
    fn apply(&self, x: &[Vec<f64>]) -> Vec<Vec<f64>> {
        let len = x[0].len() - (self.k - 1) * self.d;
        (0..self.out)
            .map(|o| {
                (0..len)
                    .map(|t| {
                        let mut s = self.b[o];
                        for i in 0..self.inp {
                            for kk in 0..self.k {
                                s += self.w[(o * self.inp + i) * self.k + kk]
                                    * x[i][t + kk * self.d];
                            }
                        }
                        s
                    })
                    .collect()
            })
            .collect()
    }
}

fn last(x: &[Vec<f64>], len: usize) -> Vec<Vec<f64>> {
    x.iter().map(|c| c[c.len() - len..].to_vec()).collect()
}

/// The trainer's forward pass in f64: valid convolutions over `x`
/// padded with `receptive_field − 1` zeros.
fn reference_wavenet(arrays: &[Array], weights: &[f64], x: &[f32]) -> Vec<f64> {
    let rf = 1 + arrays
        .iter()
        .map(|a| a.layers.iter().map(|l| (l.k - 1) * l.d).sum::<usize>() + a.head_k - 1)
        .sum::<usize>();
    let mut padded = vec![0.0; rf - 1];
    padded.extend(x.iter().map(|&v| f64::from(v)));
    let c = vec![padded.clone()];
    let mut y = c.clone();
    let mut head: Option<Vec<Vec<f64>>> = None;
    let mut ws = weights.iter();
    for a in arrays {
        let rechannel = Conv::read(&mut ws, a.input, a.channels, 1, 1, false);
        let rf_no_head = 1 + a.layers.iter().map(|l| (l.k - 1) * l.d).sum::<usize>();
        let out_no_head = y[0].len().min(c[0].len()) - (rf_no_head - 1);
        let mut xs = rechannel.apply(&y);
        for l in &a.layers {
            let mid = if matches!(l.gate, Gate::None) {
                a.bottleneck
            } else {
                2 * a.bottleneck
            };
            let conv = Conv::read(&mut ws, a.channels, mid, l.k, l.d, true);
            let mixer = Conv::read(&mut ws, 1, mid, 1, 1, false);
            let l1 = a
                .layer1x1
                .then(|| Conv::read(&mut ws, a.bottleneck, a.channels, 1, 1, true));
            let h1 = a
                .head1x1
                .map(|h| Conv::read(&mut ws, a.bottleneck, h, 1, 1, true));
            let zc = conv.apply(&xs);
            let len = zc[0].len();
            let mix = last(&mixer.apply(&c), len);
            let b = a.bottleneck;
            let act: Vec<Vec<f64>> = (0..b)
                .map(|j| {
                    (0..len)
                        .map(|t| {
                            let z = |ch: usize| zc[ch][t] + mix[ch][t];
                            match &l.gate {
                                Gate::None => l.act.f(z(j), j),
                                Gate::Gated(g) => l.act.f(z(j), j) * g.f(z(b + j), j),
                                Gate::Blended(g) => {
                                    let s = g.f(z(b + j), j);
                                    s * l.act.f(z(j), j) + (1.0 - s) * z(j)
                                }
                            }
                        })
                        .collect()
                })
                .collect();
            let layer_out = l1.as_ref().map_or_else(|| act.clone(), |c| c.apply(&act));
            let head_term = last(
                &h1.as_ref().map_or_else(|| act.clone(), |c| c.apply(&act)),
                out_no_head,
            );
            let res = last(&xs, layer_out[0].len());
            xs = res
                .iter()
                .zip(&layer_out)
                .map(|(r, o)| r.iter().zip(o).map(|(a, b)| a + b).collect())
                .collect();
            head = Some(match head {
                None => head_term,
                Some(h) => last(&h, out_no_head)
                    .iter()
                    .zip(&head_term)
                    .map(|(p, q)| p.iter().zip(q).map(|(a, b)| a + b).collect())
                    .collect(),
            });
        }
        let head_conv = Conv::read(
            &mut ws,
            a.head_width(),
            a.head_size,
            a.head_k,
            1,
            a.head_bias,
        );
        let head_out = head_conv.apply(head.as_ref().unwrap());
        y = last(&xs, head_out[0].len());
        head = Some(head_out);
    }
    let scale = *ws.next().unwrap();
    assert!(ws.next().is_none());
    head.unwrap()[0].iter().map(|v| scale * v).collect()
}

fn weight_count_wavenet(arrays: &[Array]) -> usize {
    let mut n = 0;
    for a in arrays {
        n += a.input * a.channels;
        for l in &a.layers {
            let mid = if matches!(l.gate, Gate::None) {
                a.bottleneck
            } else {
                2 * a.bottleneck
            };
            n += a.channels * mid * l.k + mid + mid;
            if a.layer1x1 {
                n += a.bottleneck * a.channels + a.channels;
            }
            if let Some(h) = a.head1x1 {
                n += a.bottleneck * h + h;
            }
        }
        n += a.head_width() * a.head_size * a.head_k + if a.head_bias { a.head_size } else { 0 };
    }
    n + 1
}

/// The arrays in 0.13-trainer keys.
fn wavenet_text(arrays: &[Array], weights: &[f64]) -> String {
    let layers: Vec<String> = arrays
        .iter()
        .map(|a| {
            let list = |f: &dyn Fn(&Layer) -> String| {
                format!("[{}]", a.layers.iter().map(f).collect::<Vec<_>>().join(","))
            };
            format!(
                r#"{{"input_size":{},"condition_size":1,"channels":{},"bottleneck":{},
                "head":{{"out_channels":{},"kernel_size":{},"bias":{}}},
                "kernel_sizes":{},"dilations":{},"activation":{},"gating_mode":{},"secondary_activation":{},
                "layer1x1":{{"active":{},"groups":1}},"head1x1":{{"active":{},"out_channels":{},"groups":1}},
                "groups_input":1,"groups_input_mixin":1,"slimmable":null,
                "conv_pre_film":{{"active":false,"shift":true,"groups":1}}}}"#,
                a.input,
                a.channels,
                a.bottleneck,
                a.head_size,
                a.head_k,
                a.head_bias,
                list(&|l| l.k.to_string()),
                list(&|l| l.d.to_string()),
                list(&|l| l.act.json()),
                list(&|l| match l.gate {
                    Gate::None => "\"none\"".into(),
                    Gate::Gated(_) => "\"gated\"".into(),
                    Gate::Blended(_) => "\"blended\"".into(),
                }),
                list(&|l| match &l.gate {
                    Gate::None => "null".into(),
                    Gate::Gated(g) | Gate::Blended(g) => g.json(),
                }),
                a.layer1x1,
                a.head1x1.is_some(),
                a.head1x1.unwrap_or(1),
            )
        })
        .collect();
    format!(
        r#"{{"version":"0.7.0","architecture":"WaveNet","sample_rate":48000,"metadata":{{}},
        "config":{{"layers":[{}],"head":null,"head_scale":0.5}},"weights":{:?}}}"#,
        layers.join(","),
        weights
    )
}

fn stimulus(r: &mut Lcg, n: usize) -> Vec<f32> {
    (0..n)
        .map(|t| {
            let tone = 0.5 * (t as f64 * 0.21).sin();
            let noise = (r.next() - 0.5) * 0.8;
            let v = if t > n - 40 { 0.0 } else { tone + noise };
            ((v * 1e4).round() / 1e4) as f32
        })
        .collect()
}

fn compare(what: &str, ours: &[f32], reference: &[f64]) -> f32 {
    let err = ours
        .iter()
        .zip(reference)
        .map(|(a, b)| (f64::from(*a) - b).abs())
        .fold(0.0, f64::max) as f32;
    let peak = reference.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    assert!(
        err <= BUDGET,
        "{what}: max abs error {err:e} (output peak {peak:.3})"
    );
    err
}

#[test]
#[cfg_attr(miri, ignore)]
fn random_wavenets_match_the_f64_reference() {
    let mut r = Lcg(0x5EED);
    let mut worst = 0.0f32;
    for case in 0..40 {
        let arrays = random_wavenet(&mut r);
        let weights: Vec<f64> = (0..weight_count_wavenet(&arrays))
            .map(|_| r.weight(0.6))
            .collect();
        let text = wavenet_text(&arrays, &weights);
        let mut net = nam::parse(&text)
            .unwrap_or_else(|e| panic!("case {case}: {e}\n{arrays:#?}"))
            .network;
        net.prepare(RATE);
        let x = stimulus(&mut r, 160);
        let ours = run(&mut net, &x);
        let reference = reference_wavenet(&arrays, &weights, &x);
        worst = worst.max(compare(
            &format!("case {case} {arrays:?}"),
            &ours,
            &reference,
        ));
    }
    println!("random WaveNets: worst max abs error {worst:.2e}");
}

fn sig(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// `.nam` LSTM in f64: per layer W (4H × (I+H)), b, h0, c0; head w, b.
fn reference_lstm(hidden: usize, layers: usize, w: &[f64], x: &[f32]) -> Vec<f64> {
    let mut ws = w.iter().copied();
    let mut cells = Vec::new();
    for l in 0..layers {
        let inp = if l == 0 { 1 } else { hidden };
        let m: Vec<f64> = ws.by_ref().take(4 * hidden * (inp + hidden)).collect();
        let b: Vec<f64> = ws.by_ref().take(4 * hidden).collect();
        let h: Vec<f64> = ws.by_ref().take(hidden).collect();
        let c: Vec<f64> = ws.by_ref().take(hidden).collect();
        cells.push((inp, m, b, h, c));
    }
    let hw: Vec<f64> = ws.by_ref().take(hidden).collect();
    let hb = ws.next().unwrap();
    assert!(ws.next().is_none());
    x.iter()
        .map(|&v| {
            let mut input = vec![f64::from(v)];
            for (inp, m, b, h, c) in &mut cells {
                let xh: Vec<f64> = input.iter().chain(h.iter()).copied().collect();
                let g: Vec<f64> = (0..4 * hidden)
                    .map(|r| {
                        b[r] + (0..*inp + hidden)
                            .map(|j| m[r * (*inp + hidden) + j] * xh[j])
                            .sum::<f64>()
                    })
                    .collect();
                for k in 0..hidden {
                    c[k] = sig(g[hidden + k]) * c[k] + sig(g[k]) * g[2 * hidden + k].tanh();
                    h[k] = sig(g[3 * hidden + k]) * c[k].tanh();
                }
                input = h.clone();
            }
            hb + hw.iter().zip(&input).map(|(a, b)| a * b).sum::<f64>()
        })
        .collect()
}

/// PyTorch GRU in f64: per layer w_ih, w_hh, b_ih, b_hh; head w, b.
fn reference_gru(hidden: usize, layers: usize, w: &[f64], x: &[f32]) -> Vec<f64> {
    let mut ws = w.iter().copied();
    let mut cells = Vec::new();
    for l in 0..layers {
        let inp = if l == 0 { 1 } else { hidden };
        let wi: Vec<f64> = ws.by_ref().take(3 * hidden * inp).collect();
        let wh: Vec<f64> = ws.by_ref().take(3 * hidden * hidden).collect();
        let bi: Vec<f64> = ws.by_ref().take(3 * hidden).collect();
        let bh: Vec<f64> = ws.by_ref().take(3 * hidden).collect();
        cells.push((inp, wi, wh, bi, bh, vec![0.0; hidden]));
    }
    let hw: Vec<f64> = ws.by_ref().take(hidden).collect();
    let hb = ws.next().unwrap();
    x.iter()
        .map(|&v| {
            let mut input = vec![f64::from(v)];
            for (inp, wi, wh, bi, bh, h) in &mut cells {
                let gi: Vec<f64> = (0..3 * hidden)
                    .map(|r| bi[r] + (0..*inp).map(|j| wi[r * *inp + j] * input[j]).sum::<f64>())
                    .collect();
                let gh: Vec<f64> = (0..3 * hidden)
                    .map(|r| bh[r] + (0..hidden).map(|j| wh[r * hidden + j] * h[j]).sum::<f64>())
                    .collect();
                let old = h.clone();
                for k in 0..hidden {
                    let rr = sig(gi[k] + gh[k]);
                    let z = sig(gi[hidden + k] + gh[hidden + k]);
                    let n = (gi[2 * hidden + k] + rr * gh[2 * hidden + k]).tanh();
                    h[k] = (1.0 - z) * n + z * old[k];
                }
                input = h.clone();
            }
            hb + hw.iter().zip(&input).map(|(a, b)| a * b).sum::<f64>()
        })
        .collect()
}

#[test]
#[cfg_attr(miri, ignore)]
fn random_recurrent_networks_match_the_f64_reference() {
    let mut r = Lcg(0xC0FFEE);
    for case in 0..12 {
        let hidden = 1 + r.below(8);
        let layers = 1 + r.below(3);
        let x = stimulus(&mut r, 300);

        let n: usize = (0..layers)
            .map(|l| {
                let inp = if l == 0 { 1 } else { hidden };
                4 * hidden * (inp + hidden) + 4 * hidden + 2 * hidden
            })
            .sum::<usize>()
            + hidden
            + 1;
        let w: Vec<f64> = (0..n).map(|_| r.weight(0.7)).collect();
        let text = format!(
            r#"{{"version":"0.7.0","architecture":"LSTM","sample_rate":48000,
            "config":{{"input_size":1,"hidden_size":{hidden},"num_layers":{layers}}},"weights":{w:?}}}"#
        );
        let mut net = nam::parse(&text).unwrap().network;
        net.prepare(RATE);
        compare(
            &format!("LSTM case {case}"),
            &run(&mut net, &x),
            &reference_lstm(hidden, layers, &w, &x),
        );

        let n: usize = (0..layers)
            .map(|l| {
                let inp = if l == 0 { 1 } else { hidden };
                3 * hidden * (inp + hidden) + 6 * hidden
            })
            .sum::<usize>()
            + hidden
            + 1;
        let w: Vec<f64> = (0..n).map(|_| r.weight(0.7)).collect();
        let text = format!(
            r#"{{"input_size":1,"hidden_size":{hidden},"num_layers":{layers},"sample_rate":48000,"weights":{w:?}}}"#
        );
        let mut net = Gru::parse(&text).unwrap();
        net.prepare(RATE);
        compare(
            &format!("GRU case {case}"),
            &run(&mut net, &x),
            &reference_gru(hidden, layers, &w, &x),
        );
    }
}
