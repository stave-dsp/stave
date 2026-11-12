//! A waveform from an external simulator (ngspice's `wrdata`: time and
//! value columns), and how far a render differs from it.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    pub time_s: Vec<f64>,
    pub value: Vec<f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum WaveformError {
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("line {line}: expected 'time value', found {found:?}")]
    MalformedLine { line: usize, found: String },
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct Difference {
    /// RMS of the difference over RMS of the waveform.
    pub normalized_rms: f64,
    pub max_abs: f64,
    pub points: usize,
}

impl Waveform {
    pub fn read(path: &Path) -> Result<Self, WaveformError> {
        let text = std::fs::read_to_string(path).map_err(|source| WaveformError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, WaveformError> {
        let mut waveform = Self {
            time_s: vec![],
            value: vec![],
        };
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let mut fields = line.split_whitespace().map(str::parse::<f64>);
            let (Some(Ok(time)), Some(Ok(value))) = (fields.next(), fields.next()) else {
                return Err(WaveformError::MalformedLine {
                    line: index + 1,
                    found: line.to_string(),
                });
            };
            waveform.time_s.push(time);
            waveform.value.push(value);
        }
        Ok(waveform)
    }

    /// How far `render`, sampled at this waveform's times, differs from it.
    pub fn difference(&self, render: &[f64]) -> Difference {
        let points = render.len().min(self.value.len());
        let (mut error, mut power, mut largest) = (0.0, 0.0, 0.0f64);
        for (rendered, expected) in render[..points].iter().zip(&self.value[..points]) {
            error += (rendered - expected).powi(2);
            power += expected * expected;
            largest = largest.max((rendered - expected).abs());
        }
        Difference {
            normalized_rms: if power > 0.0 {
                (error / power).sqrt()
            } else {
                error.sqrt()
            },
            max_abs: largest,
            points,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_time_value_columns_and_measures_the_difference() {
        let w = Waveform::parse(" 0.0  0.0\n 1.0e-3  0.5 \n\n2e-3 1.0\n").unwrap();
        assert_eq!(w.time_s, [0.0, 1e-3, 2e-3]);
        let d = w.difference(&[0.0, 0.5, 0.9]);
        assert_eq!(d.points, 3);
        assert!((d.max_abs - 0.1).abs() < 1e-12);
        assert!(matches!(
            Waveform::parse("0.0 x"),
            Err(WaveformError::MalformedLine { line: 1, .. })
        ));
    }

    #[test]
    fn a_missing_file_names_its_path() {
        let e = Waveform::read(Path::new("/nope/none.txt")).unwrap_err();
        assert!(matches!(e, WaveformError::Read { .. }));
        assert!(e.to_string().contains("/nope/none.txt"));
    }
}
