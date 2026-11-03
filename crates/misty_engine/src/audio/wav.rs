//! Tiny 16-bit PCM WAV writer, used to make test and sample clips without shipping files.

/// Mono sine tone of `seconds` at `frequency` Hz with 10 ms fades (no clicks), as WAV bytes.
pub fn sine_beep(frequency: f32, seconds: f32, sample_rate: u32) -> Vec<u8> {
    let count = (seconds * sample_rate as f32) as usize;
    let fade = (sample_rate as usize / 100).max(1);
    let samples: Vec<i16> = (0..count)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let edge = (i.min(count - 1 - i) as f32 / fade as f32).min(1.0);
            let v = (t * frequency * std::f32::consts::TAU).sin() * 0.5 * edge;
            (v * i16::MAX as f32) as i16
        })
        .collect();
    pcm16_mono(&samples, sample_rate)
}

/// Wrap mono 16-bit samples in a RIFF/WAVE header.
pub fn pcm16_mono(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beep_has_valid_header_and_length() {
        let wav = sine_beep(440.0, 0.25, 22050);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        let samples = (0.25 * 22050.0) as usize;
        assert_eq!(wav.len(), 44 + samples * 2);
        let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(data_len, samples * 2);
        assert_eq!(
            i16::from_le_bytes([wav[44], wav[45]]),
            0,
            "fades in from silence"
        );
    }

    #[test]
    fn sample_clip_matches_generator() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../SampleProject/Assets/Audio/Beep.wav");
        let expected = sample_clip();
        if std::env::var_os("MISTY_WRITE_SAMPLES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &expected).unwrap();
        }
        // Compare size and header only: `sin` may differ in the last bit between platforms.
        let found = std::fs::read(&path).unwrap_or_default();
        assert!(
            found.len() == expected.len() && found[..44] == expected[..44],
            "regenerate with MISTY_WRITE_SAMPLES=1 cargo test sample_clip"
        );
    }

    /// `SampleProject/Assets/Audio/Beep.wav`: 0.3 s at 660 Hz.
    fn sample_clip() -> Vec<u8> {
        sine_beep(660.0, 0.3, 22050)
    }
}
