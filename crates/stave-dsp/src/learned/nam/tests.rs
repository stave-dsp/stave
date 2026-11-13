use super::*;

/// A one-layer WaveNet in the legacy keys; `extra` is spliced into the
/// layer array's config.
fn legacy(extra: &str) -> String {
    format!(
        r#"{{"version": "0.5.4", "architecture": "WaveNet", "sample_rate": 44100,
        "metadata": {{"name": "Tiny", "loudness": -12.5, "input_level_dbu": 12}},
        "config": {{"layers": [{{"input_size": 1, "condition_size": 1, "channels": 1,
            "head_size": 1, "head_bias": false, "kernel_size": 1, "dilations": [1],
            "activation": "Tanh", "gated": false {extra}}}], "head": null, "head_scale": 0.5}},
        "weights": [1, 2, 0, 0, 0, 0, 1, 0.5]}}"#
    )
}

#[test]
fn reads_the_legacy_keys_and_metadata() {
    let nam = parse(&legacy("")).unwrap();
    assert_eq!(nam.version, "0.5.4");
    assert_eq!(nam.sample_rate, Some(44_100.0));
    assert_eq!(nam.metadata.name.as_deref(), Some("Tiny"));
    assert_eq!(nam.metadata.loudness, Some(-12.5));
    assert_eq!(nam.metadata.input_level_dbu, Some(12.0));
    let mut net = nam.network;
    assert_eq!(net.sample_rate(), 44_100.0);
    net.prepare(44_100.0);
    assert!((net.tick(0.25) - 0.5 * 0.5f32.tanh()).abs() < 1e-6);
}

#[test]
fn a_file_without_a_rate_is_nams_default() {
    let text = legacy("").replace(r#""sample_rate": 44100,"#, "");
    let nam = parse(&text).unwrap();
    assert_eq!(nam.sample_rate, None);
    assert_eq!(nam.network.sample_rate(), DEFAULT_SAMPLE_RATE);
}

#[test]
fn unsupported_features_are_refused_by_name() {
    let refused = |text: &str| match parse(text) {
        Err(LoadError::Unsupported(what)) => what,
        other => panic!("expected Unsupported, got {other:?}"),
    };
    assert!(refused(&legacy(r#", "conv_pre_film": {"active": true}"#)).contains("FiLM"));
    assert!(refused(&legacy(r#", "groups_input": 2"#)).contains("grouped"));
    assert!(refused(&legacy(r#", "slimmable": {"method": "x"}"#)).contains("slimmable"));
    assert!(
        refused(&legacy("").replace(r#""activation": "Tanh""#, r#""activation": "Fasttanh""#))
            .contains("Fasttanh")
    );
    assert!(
        refused(&legacy("").replace(r#""head": null"#, r#""head": {"channels": 2}"#))
            .contains("post-head")
    );
    // Inactive FiLM blocks, as the 0.13 exporter writes them, are fine.
    assert!(parse(&legacy(
        r#", "conv_pre_film": {"active": false, "shift": true, "groups": 1}"#
    ))
    .is_ok());
}

#[test]
fn versions_architectures_and_weights_are_checked() {
    let v = |from: &str, to: &str| parse(&legacy("").replace(from, to));
    assert!(matches!(v("0.5.4", "0.4.0"), Err(LoadError::Version(_))));
    assert!(matches!(v("0.5.4", "1.0.0"), Err(LoadError::Version(_))));
    assert!(matches!(v("0.5.4", "0.7"), Err(LoadError::Version(_))));
    assert!(v("0.5.4", "0.7.0").is_ok());
    assert!(matches!(
        v("\"WaveNet\"", "\"ConvNet\""),
        Err(LoadError::Architecture(_))
    ));
    assert_eq!(
        v("0, 1, 0.5]", "0, 1]").unwrap_err(),
        LoadError::Weights {
            expected: 8,
            found: 7
        }
    );
    assert!(matches!(
        parse("{\"version\": "),
        Err(LoadError::Json { .. })
    ));
    assert!(matches!(
        v("\"dilations\": [1]", "\"dilations\": [1.5]"),
        Err(LoadError::Config(_))
    ));
}

#[test]
fn lstm_files_load() {
    // One unit, zero weights: the output is the head bias.
    let mut w = vec![0.0; 4 * 2 + 4 + 2 + 1];
    w.push(0.25);
    let text = format!(
        r#"{{"version": "0.7.0", "architecture": "LSTM", "config": {{"input_size": 1,
        "hidden_size": 1, "num_layers": 1}}, "weights": {w:?}, "sample_rate": 48000}}"#
    );
    let mut net = parse(&text).unwrap().network;
    assert!(matches!(net, Network::Lstm(_)));
    net.prepare(48_000.0);
    assert_eq!(net.tick(0.7), 0.25);
    assert_eq!(net.latency(), 0);
}

#[test]
fn composes_inside_an_oversampled_region() {
    // A network trained at 96 kHz in a 48 kHz host: the 2× region
    // prepares it at its own rate, so it runs; at the host rate it refuses.
    use crate::{Oversampled2x, Serial};
    let text = legacy("").replace(r#""sample_rate": 44100"#, r#""sample_rate": 96000"#);
    let net = parse(&text).unwrap().network;
    let mut bare = net.clone();
    bare.prepare(48_000.0);
    assert!(bare.rate_mismatch().is_some());
    let mut region = Serial((Oversampled2x::front(net), ()));
    region.prepare(48_000.0);
    assert!(region.0 .0.inner.rate_mismatch().is_none());
    let peak = (0..400)
        .map(|t| region.tick(0.5 * (t as f32 * 0.05).sin()).abs())
        .fold(0.0, f32::max);
    assert!(peak > 0.1, "the region passes the network's output: {peak}");
    assert_eq!(region.latency(), Oversampled2x::front(()).latency());
}
