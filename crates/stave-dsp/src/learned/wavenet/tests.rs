use super::*;

fn plain(kernel: usize, dilation: usize) -> LayerSpec {
    LayerSpec {
        kernel,
        dilation,
        activation: Activation::Tanh,
        gating: Gating::None,
    }
}

fn array(
    input_size: usize,
    channels: usize,
    head_size: usize,
    layers: Vec<LayerSpec>,
) -> ArraySpec {
    ArraySpec {
        input_size,
        condition_size: 1,
        channels,
        bottleneck: channels,
        layers,
        layer1x1: true,
        head1x1: None,
        head_size,
        head_kernel: 1,
        head_dilation: 1,
        head_bias: true,
    }
}

/// Weights from a fixed LCG in ±0.6: deterministic and non-trivial.
fn weights(n: usize) -> Vec<f32> {
    let mut s: u32 = 12345;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((s >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 1.2
        })
        .collect()
}

/// The weight count a shape needs, read from the error a short list gets.
fn count(specs: &[ArraySpec]) -> usize {
    match WaveNet::build(specs, &[], 48_000.0) {
        Err(LoadError::Weights { expected, .. }) => expected,
        other => panic!("{other:?}"),
    }
}

fn two_arrays() -> Vec<ArraySpec> {
    vec![
        array(1, 4, 3, vec![plain(3, 1), plain(3, 2), plain(3, 4)]),
        array(4, 3, 1, vec![plain(2, 1), plain(2, 8)]),
    ]
}

#[test]
fn receptive_field_sums_every_convolution_span() {
    let specs = two_arrays();
    let net = WaveNet::build(&specs, &weights(count(&specs)), 48_000.0).unwrap();
    // 1 + (2+4+8) + (1+8) = 24.
    assert_eq!(net.receptive_field(), 24);
    assert_eq!(net.tail(), Tail::Samples(23));
}

#[test]
fn silence_after_reset_is_already_settled() {
    // Reset solves the state infinite silence leaves, so silence in
    // gives one constant out from the first sample, and that constant is
    // what a long run of silence from a zero state converges to.
    let specs = two_arrays();
    let mut net = WaveNet::build(&specs, &weights(count(&specs)), 48_000.0).unwrap();
    net.prepare(48_000.0);
    let first = net.tick(0.0);
    for _ in 0..100 {
        assert_eq!(net.tick(0.0), first);
    }
    assert!(
        first.abs() > 1e-4,
        "biases should make silence nonzero: {first}"
    );
    for _ in 0..50 {
        net.tick(0.7);
    }
    let mut last = 0.0;
    for _ in 0..net.receptive_field() {
        last = net.tick(0.0);
    }
    assert!((last - first).abs() < 1e-6, "{last} vs {first}");
}

#[test]
fn output_depends_on_exactly_the_receptive_field() {
    let specs = two_arrays();
    let mut net = WaveNet::build(&specs, &weights(count(&specs)), 48_000.0).unwrap();
    let rf = net.receptive_field();
    let quiet = net.tick(0.0);
    net.reset();
    // An impulse moves the output for rf samples, then never again.
    let mut moved = Vec::new();
    for t in 0..rf + 10 {
        let y = net.tick(if t == 0 { 0.9 } else { 0.0 });
        moved.push((y - quiet).abs() > 1e-7);
    }
    assert!(moved[rf - 1], "the impulse should reach the last tap");
    assert!(
        moved[rf..].iter().all(|m| !m),
        "nothing after the receptive field"
    );
}

#[test]
fn a_foreign_rate_is_silence_until_chosen() {
    let specs = two_arrays();
    let mut net = WaveNet::build(&specs, &weights(count(&specs)), 48_000.0).unwrap();
    net.prepare(44_100.0);
    assert_eq!(
        net.rate_mismatch(),
        Some(RateMismatch {
            trained: 48_000.0,
            prepared: 44_100.0
        })
    );
    assert_eq!(net.tick(0.5), 0.0);
    net.set_rate_policy(RatePolicy::Any);
    assert_ne!(net.tick(0.5), 0.0);
    net.prepare(48_000.0);
    assert_eq!(net.rate_mismatch(), None);
}

#[test]
fn shapes_that_do_not_chain_are_refused() {
    let mut specs = two_arrays();
    specs[1].input_size = 5;
    assert!(matches!(
        WaveNet::build(&specs, &[], 48_000.0),
        Err(LoadError::Config(_))
    ));
    let mut specs = two_arrays();
    specs[1].head_size = 2;
    assert!(matches!(
        WaveNet::build(&specs, &[], 48_000.0),
        Err(LoadError::Unsupported(_))
    ));
    let mut specs = two_arrays();
    specs[0].layer1x1 = false;
    specs[0].bottleneck = 2;
    assert!(matches!(
        WaveNet::build(&specs, &[], 48_000.0),
        Err(LoadError::Config(_))
    ));
    assert!(matches!(
        WaveNet::build(&[], &[], 48_000.0),
        Err(LoadError::Config(_))
    ));
}
