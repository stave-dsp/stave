//! The resolver against a library made on the spot: sets, manifest
//! globs, directories, sidecars and AIFF left alone, combining, excerpts,
//! the lock and drift.

use super::*;
use stave_host::wav;

struct Lib(PathBuf);

impl Drop for Lib {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tone(path: &Path, rate: u32, channels: usize, seconds: f64, amp: f32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let n = (f64::from(rate) * seconds) as usize;
    let c: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * amp).collect();
    wav::write(path, &wav::Audio { sample_rate: rate, channels: vec![c; channels] }).unwrap();
}

fn library(tag: &str) -> Lib {
    let root = std::env::temp_dir().join(format!("stave-corpus-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    tone(&root.join("Test Corpus/kicks/K1.wav"), 48_000, 1, 0.5, 0.5);
    tone(&root.join("Test Corpus/kicks/K2.wav"), 48_000, 1, 0.5, 0.5);
    std::fs::write(root.join("Test Corpus/kicks/._K1.wav"), b"not audio").unwrap();
    tone(&root.join("Stems/Song B/BASS/BASS CLEAN.wav"), 44_100, 1, 2.0, 0.25);
    tone(&root.join("Stems/Song B/GTR/GTR.wav"), 44_100, 2, 2.0, 0.25);
    tone(&root.join("Stems/Song A/GTR/GTR.wav"), 44_100, 2, 1.0, 0.25);
    std::fs::write(root.join("Stems/Song A/GTR/Locked.aif"), b"FORM....AIFCable").unwrap();
    std::fs::write(
        root.join("_corpus.json"),
        r#"{"kicks": [{"file": "Test Corpus/kicks/K2.wav"}, {"file": "Test Corpus/kicks/K1.wav"}]}"#,
    )
    .unwrap();
    let rows = [
        ("Stems/Song B/BASS/BASS CLEAN.wav", "Stems/Song B/BASS", "wav", 1, 2.0),
        ("Stems/Song B/GTR/GTR.wav", "Stems/Song B/GTR", "wav", 2, 2.0),
        ("Stems/Song A/GTR/GTR.wav", "Stems/Song A/GTR", "wav", 2, 1.0),
        ("Stems/Song A/GTR/Locked.aif", "Stems/Song A/GTR", "aif", 2, 1.0),
    ];
    let mut csv = String::from("path,category,source,kind,format,rate,channels,bits,seconds,bpm,key,size,mtime\n");
    for (p, c, f, ch, s) in rows {
        csv += &format!("{p},{c},Stems,other,{f},44100,{ch},24,{s},,,1,1\n");
    }
    std::fs::write(root.join("_manifest.csv"), csv).unwrap();
    Lib(root)
}

const SPEC: &str = "\
role kicks
  about k
  used-by C13
  required yes
  select set kicks
  count 5
role bass
  about b
  used-by C13
  required no
  select manifest Stems/*/BASS; name=*CLEAN*.wav
  count 1
  excerpt 0.5 1
role mix
  about m
  used-by C13
  required yes
  select manifest Stems/*
  combine sum 2
  count 5
role di
  about d
  used-by C13
  required no
  select dir DI/Guitar; min-seconds=1
  count 4
  fill record some
";

#[test]
fn roles_resolve_in_order_without_sidecars_or_aiff() {
    let lib = library("resolve");
    let c = Corpus::at(&lib.0, Spec::parse(SPEC).unwrap(), None);
    let names = |r: &str| c.role(r).unwrap().iter().map(|i| i.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("kicks"), ["K2", "K1"], "the set's own order");
    assert_eq!(names("bass"), ["BASS CLEAN"]);
    // Grouped by song, songs in path order; the AIFF never picked.
    let mix = c.role("mix").unwrap();
    assert_eq!(mix.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Song A", "Song B"]);
    assert_eq!(mix[0].files.len(), 1);
    assert_eq!(mix[1].files.len(), 2);
    let Err(Skip(why)) = c.role("di") else { panic!("no DI in this library") };
    assert!(why.contains("role di has no material") && why.contains("record some"), "{why}");
    // Without _corpus.json a set is its directory.
    std::fs::remove_file(lib.0.join("_corpus.json")).unwrap();
    let c = Corpus::at(&lib.0, Spec::parse(SPEC).unwrap(), None);
    let kicks: Vec<_> = c.role("kicks").unwrap().into_iter().map(|i| i.name).collect();
    assert_eq!(kicks, ["K1", "K2"]);
}

#[test]
fn items_load_summed_and_excerpted() {
    let lib = library("load");
    let c = Corpus::at(&lib.0, Spec::parse(SPEC).unwrap(), None);
    let bass = c.load(&c.role("bass").unwrap()[0]).unwrap();
    assert_eq!((bass.rate, bass.channels.len(), bass.channels[0].len()), (44_100, 1, 44_100));
    // The excerpt starts half a second in.
    let full = wav::read(&lib.0.join("Stems/Song B/BASS/BASS CLEAN.wav")).unwrap();
    assert_eq!(bass.channels[0][..64], full.channels[0][22_050..22_050 + 64]);
    // A mono bass and a stereo guitar sum on both channels.
    let mix = c.load(&c.role("mix").unwrap()[1]).unwrap();
    assert_eq!(mix.channels.len(), 2);
    let x = full.channels[0][1000];
    assert!((mix.channels[1][1000] - 2.0 * x).abs() < 1e-6);
    assert!(mix.files.iter().all(|f| f.1.is_none()), "unlocked: not hashed");
}

#[test]
fn the_lock_pins_files_and_drift_fails() {
    let lib = library("lock");
    let spec = Spec::parse(SPEC).unwrap();
    let path = lib.0.join("Test Corpus/kicks/K1.wav");
    let bytes = std::fs::read(&path).unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    let digest = &spec.role("kicks").unwrap().digest;
    let text = format!(
        "# lock\nrole\tkicks\t{digest}\nitem\tkicks\t0\t{}\t{}\t{}\tTest Corpus/kicks/K1.wav\nrole\tbass\tstale\n",
        sha256::hex(&bytes),
        bytes.len(),
        mtime(&meta)
    );
    let c = Corpus::at(&lib.0, spec.clone(), Some(Lock::parse(&text)));
    let kicks = c.role("kicks").unwrap();
    assert_eq!(kicks.len(), 1, "the lock's picks, not the set's two");
    let m = c.load(&kicks[0]).unwrap();
    assert_eq!(m.files[0].1.as_deref(), Some(sha256::hex(&bytes).as_str()));
    assert!(m.describe().starts_with("kicks/K1"), "{}", m.describe());
    // A lock under other rules is ignored: bass resolves live.
    assert!(!c.locked(spec.role("bass").unwrap()));
    assert_eq!(c.role("bass").unwrap().len(), 1);
    // The file changes under the lock.
    tone(&path, 48_000, 1, 0.6, 0.4);
    let e = c.load(&kicks[0]).unwrap_err();
    assert!(e.contains("corpus drift") && e.contains("K1.wav"), "{e}");
}

#[test]
fn config_values_parse_like_the_scripts() {
    let text = "# c\nSTAVE_LINUX_HOST=x\nSTAVE_SAMPLE_LIBRARY=\"/a b/lib\"\n";
    assert_eq!(config_value(text, "STAVE_SAMPLE_LIBRARY").as_deref(), Some("/a b/lib"));
    assert_eq!(config_value("export K='v'", "K").as_deref(), Some("v"));
    assert_eq!(config_value("KEY2=1", "KEY"), None);
    assert_eq!(config_value("K=1\nK=2\n", "K").as_deref(), Some("2"), "the last assignment wins");
}
