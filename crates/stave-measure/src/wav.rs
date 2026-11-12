//! WAV in and out (planar f32 channels).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

#[derive(Debug, thiserror::Error)]
pub enum WavError {
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: hound::Error,
    },
    #[error("{}: the header declares no channels", path.display())]
    NoChannels { path: PathBuf },
    #[error("{}: {bits}-bit integer samples are not supported", path.display())]
    UnsupportedBitDepth { path: PathBuf, bits: u16 },
    #[error("{channels} channels do not fit a WAV header")]
    TooManyChannels { channels: usize },
    #[error("channels have different lengths {lengths:?}")]
    MismatchedChannelLengths { lengths: Vec<usize> },
}

/// Read any PCM/float WAV into planar f32 in [−1, 1].
pub fn read(path: &Path) -> Result<Audio, WavError> {
    read_range(path, 0, usize::MAX)
}

/// What a WAV holds, from its header alone (nothing decoded).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: usize,
}

/// The header of a WAV: rate, channels, length in frames.
pub fn probe(path: &Path) -> Result<Info, WavError> {
    let reader = open(path)?;
    let spec = reader.spec();
    Ok(Info {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        frames: reader.duration() as usize,
    })
}

/// Read at most `frames` frames starting at frame `start` (clamped to
/// the file), seeking past the rest. Memory is bounded by the window,
/// not the file — a multi-hour render costs what its slice costs.
pub fn read_range(path: &Path, start: usize, frames: usize) -> Result<Audio, WavError> {
    let read_failed = |source| WavError::Read {
        path: path.to_path_buf(),
        source,
    };
    let mut reader = open(path)?;
    let spec = reader.spec();
    let total = reader.duration() as usize;
    let start = start.min(total);
    let frames = frames.min(total - start);
    // `start ≤ duration`, which is a u32.
    reader
        .seek(start as u32)
        .map_err(|e| read_failed(hound::Error::IoError(e)))?;
    let n = usize::from(spec.channels);
    let count = frames * n;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().take(count).collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            if !(1..=32).contains(&bits) {
                return Err(WavError::UnsupportedBitDepth {
                    path: path.to_path_buf(),
                    bits,
                });
            }
            let scale = 1.0 / (1i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .take(count)
                .map(|s| s.map(|v| v as f32 * scale))
                .collect()
        }
    }
    .map_err(read_failed)?;
    let mut channels = vec![Vec::with_capacity(interleaved.len() / n); n];
    for (i, s) in interleaved.into_iter().enumerate() {
        channels[i % n].push(s);
    }
    Ok(Audio {
        sample_rate: spec.sample_rate,
        channels,
    })
}

fn open(path: &Path) -> Result<hound::WavReader<std::io::BufReader<std::fs::File>>, WavError> {
    let reader = hound::WavReader::open(path).map_err(|source| WavError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if reader.spec().channels == 0 {
        return Err(WavError::NoChannels {
            path: path.to_path_buf(),
        });
    }
    Ok(reader)
}

/// Write planar channels as 32-bit float WAV.
pub fn write(path: &Path, audio: &Audio) -> Result<(), WavError> {
    let lengths: Vec<usize> = audio.channels.iter().map(Vec::len).collect();
    if lengths.windows(2).any(|w| w[0] != w[1]) {
        return Err(WavError::MismatchedChannelLengths { lengths });
    }
    let channels = u16::try_from(audio.channels.len())
        .ok()
        .filter(|&c| c > 0)
        .ok_or(WavError::TooManyChannels {
            channels: audio.channels.len(),
        })?;
    let spec = hound::WavSpec {
        channels,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let write_failed = |source| WavError::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(write_failed)?;
    for frame in 0..lengths.first().copied().unwrap_or(0) {
        for channel in &audio.channels {
            writer.write_sample(channel[frame]).map_err(write_failed)?;
        }
    }
    writer.finalize().map_err(write_failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A uniquely named scratch directory, removed on drop.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let p =
                std::env::temp_dir().join(format!("stave-measure-{tag}-{}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn float_round_trip_is_exact() {
        let dir = TempDir::new("wav");
        let p = dir.path().join("x.wav");
        let a = Audio {
            sample_rate: 44_100,
            channels: vec![vec![0.5, -1.0, 0.25], vec![0.0; 3]],
        };
        write(&p, &a).unwrap();
        assert_eq!(read(&p).unwrap(), a);
    }

    #[test]
    fn int_pcm_is_normalized() {
        let dir = TempDir::new("wav-int");
        let p = dir.path().join("i.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for v in [i16::MIN, 0, 16_384] {
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
        assert_eq!(read(&p).unwrap().channels, vec![vec![-1.0, 0.0, 0.5]]);
    }

    #[test]
    fn a_range_reads_only_its_window() {
        let dir = TempDir::new("range");
        let p = dir.path().join("r.wav");
        let ramp: Vec<f32> = (0..100).map(|i| i as f32 / 128.0).collect();
        let a = Audio {
            sample_rate: 48_000,
            channels: vec![ramp.clone(), ramp.iter().map(|v| -v).collect()],
        };
        write(&p, &a).unwrap();
        let info = probe(&p).unwrap();
        assert_eq!((info.sample_rate, info.channels, info.frames), (48_000, 2, 100));
        let r = read_range(&p, 40, 10).unwrap();
        assert_eq!(r.channels[0], ramp[40..50]);
        assert_eq!(r.channels[1][0], -ramp[40]);
        // Clamped at the end, and past it.
        assert_eq!(read_range(&p, 95, 10).unwrap().channels[0], ramp[95..]);
        assert!(read_range(&p, 500, 10).unwrap().channels[0].is_empty());
    }

    #[test]
    fn missing_file_names_the_path() {
        let e = read(Path::new("/nope/none.wav")).unwrap_err();
        assert!(matches!(e, WavError::Read { .. }), "{e:?}");
        assert!(e.to_string().contains("/nope/none.wav"), "{e}");
    }

    #[test]
    fn refuses_to_write_what_a_wav_cannot_hold() {
        let dir = TempDir::new("refuse");
        let mismatched = Audio {
            sample_rate: 48_000,
            channels: vec![vec![0.0; 4], vec![0.0; 3]],
        };
        assert!(matches!(
            write(&dir.path().join("r.wav"), &mismatched),
            Err(WavError::MismatchedChannelLengths { .. })
        ));
        let empty = Audio {
            sample_rate: 48_000,
            channels: vec![],
        };
        assert!(matches!(
            write(&dir.path().join("e.wav"), &empty),
            Err(WavError::TooManyChannels { channels: 0 })
        ));
    }
}
