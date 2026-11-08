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
    pcm16(samples, 1, sample_rate)
}

/// Stereo 16-bit WAV from (left, right) samples in -1..1.
pub fn pcm16_stereo(frames: &[(f32, f32)], sample_rate: u32) -> Vec<u8> {
    let q = |v: f32| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
    let samples: Vec<i16> = frames.iter().flat_map(|(l, r)| [q(*l), q(*r)]).collect();
    pcm16(&samples, 2, sample_rate)
}

/// Mono WAV of `f(t)` (-1..1) for `seconds`.
pub fn synth(seconds: f32, sample_rate: u32, mut f: impl FnMut(f32) -> f32) -> Vec<u8> {
    let n = (seconds * sample_rate as f32) as usize;
    let samples: Vec<i16> = (0..n)
        .map(|i| (f(i as f32 / sample_rate as f32).clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    pcm16_mono(&samples, sample_rate)
}

/// Interleaved 16-bit samples with `channels` channels in a RIFF/WAVE header.
fn pcm16(samples: &[i16], channels: u16, sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2 * channels as u32).to_le_bytes());
    out.extend_from_slice(&(2 * channels).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Length in seconds of a WAV file's audio (`None` for other formats or a broken header).
pub fn duration_of(path: &std::path::Path) -> Option<f32> {
    duration_of_bytes(&crate::vfs::read(path).ok()?)
}

/// [`duration_of`] for WAV bytes: walks the RIFF chunks for `fmt ` (byte rate) and `data`.
pub fn duration_of_bytes(b: &[u8]) -> Option<f32> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let u32_at = |i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let (mut i, mut byte_rate, mut data) = (12, None, None);
    while i + 8 <= b.len() {
        let len = u32_at(i + 4)? as usize;
        match &b[i..i + 4] {
            b"fmt " => byte_rate = u32_at(i + 16),
            b"data" => data = Some(len),
            _ => {}
        }
        i += 8 + len + (len & 1);
    }
    let rate = byte_rate.filter(|r| *r > 0)?;
    Some(data? as f32 / rate as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_reads_the_header() {
        let d = duration_of_bytes(&sine_beep(440.0, 0.5, 8000)).unwrap();
        assert!((d - 0.5).abs() < 1e-3);
        assert_eq!(duration_of_bytes(b"not a wav"), None);
    }

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
