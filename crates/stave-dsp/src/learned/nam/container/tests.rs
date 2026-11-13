use super::*;
use crate::learned::error::LoadError;
use crate::learned::nam::parse;

/// A one-layer WaveNet written by hand: y = s·tanh(2x). `rate` is the
/// `sample_rate` field (none when `None`).
fn wavenet(s: f32, rate: Option<u32>) -> String {
    let rate = rate.map_or(String::new(), |r| format!(r#""sample_rate": {r},"#));
    format!(
        r#"{{"version": "0.7.0", "architecture": "WaveNet", {rate}
        "config": {{"layers": [{{"input_size": 1, "condition_size": 1, "channels": 1,
            "head_size": 1, "head_bias": false, "kernel_size": 1, "dilations": [1],
            "activation": "Tanh", "gated": false}}], "head": null, "head_scale": {s}}},
        "weights": [1, 2, 0, 0, 0, 0, 1, {s}]}}"#
    )
}

/// A one-unit LSTM written by hand (the module example's): c' = c +
/// 0.5·tanh(x), y = h.
fn lstm(rate: Option<u32>) -> String {
    let rate = rate.map_or(String::new(), |r| format!(r#""sample_rate": {r},"#));
    format!(
        r#"{{"version": "0.7.0", "architecture": "LSTM", {rate}
        "config": {{"input_size": 1, "hidden_size": 1, "num_layers": 1}},
        "weights": [0, 0,  0, 0,  1, 0,  0, 0,   0, 9, 0, 9,  0, 0,  1, 0]}}"#
    )
}

/// A container of `(max_value, model)` entries, written verbatim.
fn container(entries: &[(&str, String)]) -> String {
    let list: Vec<String> = entries
        .iter()
        .map(|(m, model)| format!(r#"{{"max_value": {m}, "model": {model}}}"#))
        .collect();
    format!(
        r#"{{"version": "0.7.0", "architecture": "SlimmableContainer",
        "config": {{"submodels": [{}]}}}}"#,
        list.join(", ")
    )
}

/// Three submodels: two WaveNets and an LSTM, breakpoints 0.25 and 0.5.
fn three() -> String {
    container(&[
        ("0.25", wavenet(0.25, Some(48_000))),
        ("0.5", wavenet(0.5, Some(48_000))),
        ("1.0", lstm(Some(48_000))),
    ])
}

fn the_container(text: &str) -> Container {
    match parse(text).unwrap().network {
        Network::Container(c) => c,
        other => panic!("expected a container, got {other:?}"),
    }
}

fn config_error(text: &str) -> String {
    match parse(text) {
        Err(LoadError::Config(what)) => what,
        other => panic!("expected a Config error, got {other:?}"),
    }
}

/// `n` samples of a test input through `stage`, from its prepared state.
fn run(stage: &mut impl Stage, n: usize) -> Vec<f32> {
    (0..n)
        .map(|t| stage.tick(0.6 * (t as f32 * 0.07).sin()))
        .collect()
}

#[test]
fn selects_by_max_value_with_exact_boundaries() {
    let c = the_container(&three());
    assert_eq!(c.max_values(), &[0.25, 0.5, 1.0]);
    assert_eq!(c.breakpoints(), &[0.25, 0.5]);
    for (value, index) in [
        (-1.0, 0),
        (0.0, 0),
        (0.249_999_999, 0),
        (0.25, 1), // exactly at a max_value: the next submodel
        (0.499_999_999, 1),
        (0.5, 2),
        (0.99, 2),
        (1.0, 2), // at the last max_value: still the last
        (7.0, 2),
    ] {
        assert_eq!(c.size_for(value), index, "size value {value}");
    }
    // A last max_value above 1: 1 selects it, as anything past it does.
    let c = the_container(&container(&[
        ("0.5", wavenet(0.25, None)),
        ("1.5", wavenet(0.5, None)),
    ]));
    assert_eq!((c.size_for(1.0), c.size_for(2.0)), (1, 1));
}

#[test]
fn defaults_to_full_size() {
    let nam = parse(&three()).unwrap();
    let Network::Container(c) = &nam.network else {
        panic!("a container");
    };
    assert_eq!(c.active(), 2);
    assert_eq!(c.submodels().len(), 3);
    assert_eq!(nam.network.multiply_adds(), nam.sizes()[2].multiply_adds);
    let ratios: Vec<f32> = nam.sizes().iter().map(|s| s.ratio).collect();
    assert_eq!(ratios, [0.0, 0.25, 0.5]);
    assert_eq!(nam.network.breakpoints(), &[0.25, 0.5]);
    // Each size's ratio selects that size through NAM's slider.
    for (i, size) in nam.sizes().iter().enumerate() {
        assert_eq!(c.size_for(f64::from(size.ratio)), i);
        assert_eq!(nam.slimmed(size.ratio).multiply_adds(), size.multiply_adds);
    }
}

/// Inside the container, every submodel runs exactly as the same model
/// loaded alone, at the default size and after any switch.
#[test]
fn switching_resets_the_chosen_submodel() {
    let models = [
        wavenet(0.25, Some(48_000)),
        wavenet(0.5, Some(48_000)),
        lstm(Some(48_000)),
    ];
    let alone = |i: usize, n: usize| {
        let mut net = parse(&models[i]).unwrap().network;
        net.prepare(48_000.0);
        run(&mut net, n)
    };
    let mut net = parse(&three()).unwrap().network;
    net.prepare(48_000.0);
    assert_eq!(run(&mut net, 64), alone(2, 64), "the default");
    for (value, i) in [(0.0, 0), (0.3, 1), (1.0, 2), (0.25, 1), (0.1, 0)] {
        // Run the current submodel on, so the one switched to has
        // state of its own from before to forget.
        run(&mut net, 37);
        net.set_size(value);
        assert_eq!(run(&mut net, 64), alone(i, 64), "after set_size({value})");
    }
    // The same size again changes nothing: the submodel runs on.
    let mut again = net.clone();
    again.set_size(0.05);
    assert_eq!(run(&mut again, 16), run(&mut net, 16));
    // Any other network ignores a size.
    let mut plain = parse(&models[0]).unwrap().network;
    plain.set_size(0.0);
    assert!(plain.breakpoints().is_empty());
}

#[test]
fn malformed_containers_are_refused() {
    let w = || wavenet(0.5, None);
    assert!(config_error(&container(&[])).contains("non-empty"));
    assert!(config_error(
        r#"{"version": "0.7.0", "architecture": "SlimmableContainer",
            "config": {"submodels": {"max_value": 1}}}"#
    )
    .contains("non-empty"));
    assert!(config_error(&three().replacen(r#""max_value": 0.25,"#, "", 1)).contains("max_value"));
    assert!(config_error(&container(&[("\"half\"", w())])).contains("max_value"));
    assert!(config_error(&container(&[("0.5", w()), ("0.5", w())])).contains("ascending"));
    assert!(config_error(&container(&[("0.7", w()), ("0.5", w())])).contains("ascending"));
    let short = config_error(&container(&[("0.5", w()), ("0.99", w())]));
    assert!(short.contains("at least 1"), "{short}");
    assert!(config_error(&container(&[("1", "3".into())])).contains("version"));
    assert!(config_error(&three().replacen(
        r#""config": {"submodels""#,
        r#""weights": [], "config": {"submodels""#,
        1
    ))
    .contains("no weights"));
    // A submodel's own error says where the submodel is.
    let at = config_error(&container(&[
        ("0.5", w()),
        (
            "1",
            w().replace(r#""weights": [1, 2"#, r#""weights": ["x", 2"#),
        ),
    ]));
    assert!(at.contains("config.submodels[1].model"), "{at}");
}

#[test]
fn unsupported_submodels_are_named_with_their_path() {
    let convnet = wavenet(0.5, None).replace(r#""WaveNet""#, r#""ConvNet""#);
    match parse(&container(&[("0.5", wavenet(0.5, None)), ("1", convnet)])) {
        Err(LoadError::Architecture(what)) => {
            assert!(what.contains("config.submodels[1].model"), "{what}");
            assert!(what.contains("ConvNet"), "{what}");
        }
        other => panic!("expected Architecture, got {other:?}"),
    }
    match parse(&container(&[("1", three())])) {
        Err(LoadError::Unsupported(what)) => {
            assert!(what.contains("config.submodels[0].model"), "{what}");
            assert!(what.contains("inside a SlimmableContainer"), "{what}");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn submodel_rates_must_agree() {
    let mismatch = config_error(&container(&[
        ("0.5", wavenet(0.5, Some(48_000))),
        ("1", lstm(Some(44_100))),
    ]));
    assert!(
        mismatch.contains("44100") && mismatch.contains("48000"),
        "{mismatch}"
    );
    assert!(mismatch.contains("config.submodels[1].model"), "{mismatch}");
    // The container's own rate counts too.
    let own = three().replacen(
        r#""config": {"submodels""#,
        r#""sample_rate": 96000, "config": {"submodels""#,
        1,
    );
    assert!(config_error(&own).contains("the container"));
    // An unrecorded rate agrees with any: it runs at the recorded one.
    let nam = parse(&container(&[
        ("0.5", wavenet(0.5, None)),
        ("1", lstm(Some(44_100))),
    ]))
    .unwrap();
    assert_eq!(nam.sample_rate, Some(44_100.0));
    assert_eq!(nam.network.sample_rate(), 44_100.0);
    let Network::Container(c) = &nam.network else {
        panic!("a container");
    };
    assert!(c.submodels().iter().all(|n| n.sample_rate() == 44_100.0));
    // None recorded anywhere: NAM's default, as for a plain file.
    let nam = parse(&container(&[("1", wavenet(0.5, None))])).unwrap();
    assert_eq!(nam.sample_rate, None);
    assert_eq!(
        nam.network.sample_rate(),
        crate::learned::nam::DEFAULT_SAMPLE_RATE
    );
}

#[test]
fn prepares_every_submodel_and_reports_the_running_one() {
    let mut net = parse(&three()).unwrap().network;
    net.prepare(44_100.0);
    assert!(net.rate_mismatch().is_some());
    assert_eq!(net.tick(0.5), 0.0); // strict: silent at a foreign rate
    net.prepare(48_000.0);
    net.set_size(0.0);
    assert!(
        net.rate_mismatch().is_none(),
        "the switched-to submodel is prepared too"
    );
    assert!(net.tick(0.5) != 0.0);
    // The LSTM's infinite tail is the container's, whichever runs.
    assert_eq!(net.tail(), Tail::Infinite);
    assert_eq!(net.latency(), 0);
}
