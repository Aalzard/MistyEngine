"""Prepare the sample project's ambience, weather and footstep sounds.

Usage::

    python tools/sample_sounds.py <sfx_dir> <project_dir>

``sfx_dir`` holds the source recordings (48 kHz 16-bit stereo WAV, made with
ElevenLabs for the project): five 30 s loops and ``footsteps_grass_10s_a.wav``.
The script writes, under ``<project_dir>/Assets/Audio``:

- ``Ambience/meadow_ambience_loop.ogg``, ``forest_birds_loop.ogg``,
  ``breeze_grass_loop.ogg`` and ``Weather/rain_steady_loop.ogg``,
  ``storm_heavy_rain_loop.ogg``: each loop is checked for a click at its seam
  (high-passed energy over the seam against its neighbourhood); a loop that
  clicks gets a 40 ms equal-power crossfade there.  All loops are brought to
  the same loudness (-24 LUFS, ITU-R BS.1770 gating) under a -1 dBFS look-ahead
  limiter run circularly, so the seam stays seamless, and are encoded as Ogg
  Vorbis (libvorbis through PyAV), then decoded again to check the length and
  the seam survived.
- ``Footsteps/footstep_grass_01..06.wav``: the six clearest steps of the
  footsteps recording (onsets from a high-passed envelope), cut at the onset,
  trimmed where the step has died away, with short fades, folded to mono (the
  recording is nearly mono) and levelled to one loudness.

Meta files (GUID and import settings) are written for new files only, so
running it again keeps the GUIDs.  Needs numpy, scipy and av (PyAV).
"""

from __future__ import annotations

import argparse
import json
import math
import uuid
import wave
from pathlib import Path

import av
import numpy as np
from scipy import signal

RATE = 48_000
LOOPS = [
    # (source, folder, name, bit rate)
    ("meadow_ambience_loop_30s.wav", "Ambience", "meadow_ambience_loop", 112_000),
    ("forest_birds_loop_30s.wav", "Ambience", "forest_birds_loop", 112_000),
    ("breeze_grass_loop_30s.wav", "Ambience", "breeze_grass_loop", 112_000),
    ("rain_steady_loop_30s.wav", "Weather", "rain_steady_loop", 128_000),
    ("storm_heavy_rain_loop_30s.wav", "Weather", "storm_heavy_rain_loop", 128_000),
]
LOOP_LUFS = -24.0
CEILING_DB = -1.0
STEPS = 6
STEP_LUFS = -20.0
# A seam whose high-passed energy is this many times its neighbourhood's median clicks.
CLICK_RATIO = 4.0


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path), "rb") as w:
        assert w.getsampwidth() == 2 and w.getframerate() == RATE, path
        raw = w.readframes(w.getnframes())
        ch = w.getnchannels()
    return (np.frombuffer(raw, dtype="<i2").astype(np.float64) / 32768.0).reshape(-1, ch)


def write_wav(path: Path, x: np.ndarray) -> None:
    pcm = np.clip(np.round(x * 32767.0), -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(x.shape[1])
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())


def k_weight(x: np.ndarray) -> np.ndarray:
    """ITU-R BS.1770 K-weighting: the high shelf and the RLB high-pass."""
    f0, gain, q = 1681.974450955533, 3.999843853973347, 0.7071752369554196
    k = math.tan(math.pi * f0 / RATE)
    vh, vb = 10 ** (gain / 20), 10 ** (gain / 20) ** 0.4996667741545416
    a0 = 1 + k / q + k * k
    b1 = [(vh + vb * k / q + k * k) / a0, 2 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0]
    a1 = [1, 2 * (k * k - 1) / a0, (1 - k / q + k * k) / a0]
    f0, q = 38.13547087602444, 0.5003270373238773
    k = math.tan(math.pi * f0 / RATE)
    d = 1 + k / q + k * k
    a2 = [1, 2 * (k * k - 1) / d, (1 - k / q + k * k) / d]
    return signal.lfilter([1, -2, 1], a2, signal.lfilter(b1, a1, x, axis=0), axis=0)


def lufs(x: np.ndarray) -> float:
    """Integrated loudness (gated); short clips use a single block."""
    y = k_weight(x)
    block, hop = int(0.4 * RATE), int(0.1 * RATE)
    if len(y) <= block:
        return 10 * math.log10(np.mean(y**2, axis=0).sum() + 1e-12) - 0.691
    ms = np.array([np.mean(y[s : s + block] ** 2, axis=0).sum() for s in range(0, len(y) - block, hop)])
    ab = ms[ms > 10 ** ((-70 + 0.691) / 10)]
    rel = 10 * math.log10(ab.mean()) - 0.691 - 10
    gated = ab[ab > 10 ** ((rel + 0.691) / 10)]
    return 10 * math.log10(gated.mean()) - 0.691


def seam_ratio(x: np.ndarray) -> float:
    """High-passed energy right over the seam (2 ms either side) against the median of the
    100 ms around it: ~1 for a seamless loop, far above for a click."""
    pad, w = int(0.1 * RATE), int(0.002 * RATE)
    joined = np.concatenate([x[-pad:], x[:pad]])
    hp = signal.butter(4, 3000, "highpass", fs=RATE, output="sos")
    y = signal.sosfiltfilt(hp, joined, axis=0)
    e = np.array([np.mean(y[s : s + w] ** 2) for s in range(0, len(y) - w, w)])
    mid = len(e) // 2
    return float(e[mid - 1 : mid + 1].max() / np.median(e))


def crossfade_seam(x: np.ndarray, seconds: float = 0.04) -> np.ndarray:
    """Equal-power crossfade of the last `seconds` into the first: the loop gets that much
    shorter and its end now flows into its start."""
    n = int(seconds * RATE)
    t = (np.arange(n) / n)[:, None]
    head, tail = x[:n], x[-n:]
    out = x[:-n].copy()
    out[:n] = head * np.sin(t * math.pi / 2) + tail * np.cos(t * math.pi / 2)
    return out


def limit(x: np.ndarray, ceiling: float, circular: bool) -> np.ndarray:
    """Look-ahead peak limiter (5 ms look-ahead/attack, 80 ms release). `circular` treats the
    clip as a loop so the gain is continuous over the seam."""
    look, release = int(0.005 * RATE), int(0.08 * RATE)
    n = len(x)
    src = np.concatenate([x, x, x]) if circular else x
    peak = np.abs(src).max(axis=1)
    need = np.minimum(1.0, ceiling / np.maximum(peak, 1e-9))
    # Gain reaches its target `look` samples early (moving minimum) ...
    from scipy.ndimage import minimum_filter1d, uniform_filter1d

    g = minimum_filter1d(need, size=2 * look + 1, mode="wrap" if circular else "nearest")
    # ... and recovers slowly: a one-pole release on the way up.
    a = math.exp(-1.0 / release)
    out = np.empty_like(g)
    cur = g[0]
    for i, v in enumerate(g):
        cur = v if v < cur else v + (cur - v) * a
        out[i] = cur
    out = uniform_filter1d(out, size=look, mode="wrap" if circular else "nearest")
    out = np.minimum(out, g)  # never above the look-ahead minimum
    y = src * out[:, None]
    return y[n : 2 * n] if circular else y


def encode_ogg(path: Path, x: np.ndarray, bit_rate: int) -> None:
    with av.open(str(path), "w", format="ogg") as f:
        s = f.add_stream("libvorbis", rate=RATE, layout="stereo" if x.shape[1] == 2 else "mono")
        s.bit_rate = bit_rate
        frame_len = 1024
        pcm = np.ascontiguousarray(x.T.astype(np.float32))
        for i in range(0, pcm.shape[1], frame_len):
            fr = av.AudioFrame.from_ndarray(pcm[:, i : i + frame_len], format="fltp",
                                            layout=s.layout.name)
            fr.sample_rate = RATE
            fr.pts = i
            for p in s.encode(fr):
                f.mux(p)
        for p in s.encode(None):
            f.mux(p)


def decode_ogg(path: Path) -> np.ndarray:
    chunks = []
    with av.open(str(path)) as f:
        for fr in f.decode(audio=0):
            chunks.append(fr.to_ndarray().astype(np.float64))
    return np.concatenate(chunks, axis=1).T


def write_meta(path: Path, settings: dict | None = None) -> None:
    meta = path.with_name(path.name + ".meta")
    if meta.exists():
        return
    body = {"guid": uuid.uuid4().hex}
    if settings:
        body.update(settings)
    meta.write_text(json.dumps(body, indent=2, sort_keys=True) + "\n", newline="\n")


def loops(sfx: Path, audio: Path) -> None:
    for src, folder, name, bit_rate in LOOPS:
        x = read_wav(sfx / src)
        before = seam_ratio(x)
        fixed = before > CLICK_RATIO
        if fixed:
            x = crossfade_seam(x)
        loud = lufs(x)
        x = x * 10 ** ((LOOP_LUFS - loud) / 20)
        x = limit(x, 10 ** (CEILING_DB / 20), circular=True)
        out = audio / folder / f"{name}.ogg"
        out.parent.mkdir(parents=True, exist_ok=True)
        write_meta(out.parent)
        encode_ogg(out, x, bit_rate)
        write_meta(out, {"force_to_mono": False, "load_type": "CompressedInMemory"})
        back = decode_ogg(out)
        print(
            f"{name:24s} {len(x) / RATE:6.3f} s  seam {before:4.2f}"
            f"{' -> crossfaded' if fixed else ' (no click)'}  {loud:6.1f} -> {lufs(x):6.1f} LUFS"
            f"  peak {20 * math.log10(np.abs(x).max()):5.1f} dBFS"
            f"  ogg {out.stat().st_size / 1024:6.0f} KiB, decoded {len(back)} frames"
            f" (source {len(x)}), decoded seam {seam_ratio(back):4.2f}"
        )
        assert len(back) == len(x), "the encoded loop has a different length"
        assert seam_ratio(back) < CLICK_RATIO, "the encoded loop clicks at its seam"


def footsteps(sfx: Path, audio: Path) -> None:
    x = read_wav(sfx / "footsteps_grass_10s_a.wav")
    mono = x.mean(axis=1)
    hp = signal.butter(4, 800, "highpass", fs=RATE, output="sos")
    y = signal.sosfiltfilt(hp, mono)
    hop = int(0.005 * RATE)
    env = np.sqrt(np.convolve(y**2, np.ones(hop) / hop, mode="same"))[::hop]
    db = 20 * np.log10(env + 1e-9)
    base = np.array([np.median(db[max(0, i - 12) : i - 2]) if i > 3 else db[0] for i in range(len(db))])
    peaks, _ = signal.find_peaks(db - base, height=8, distance=int(0.3 / 0.005))
    floor = np.percentile(db, 20)
    # Real steps: loud (well above the floor), after the silent lead-in.
    steps = [p for p in peaks if p * hop > 0.5 * RATE and db[p] > floor + 15]
    best = sorted(sorted(steps, key=lambda p: -db[p])[:STEPS])
    print(f"onsets at {[round(float(p * hop / RATE), 2) for p in steps]} s; using {[round(float(p * hop / RATE), 2) for p in best]}")
    out_dir = audio / "Footsteps"
    out_dir.mkdir(parents=True, exist_ok=True)
    write_meta(out_dir)
    onsets = [p * hop for p in steps]
    for i, p in enumerate(best, 1):
        start = max(0, p * hop - int(0.012 * RATE))
        later = [o for o in onsets if o > p * hop + int(0.1 * RATE)]
        end = min(p * hop + int(0.42 * RATE), (later[0] - int(0.03 * RATE)) if later else len(mono))
        # Trim the tail where the step has died away (35 dB under its peak, or the floor).
        k0, k1 = start // hop, end // hop
        peak_db = db[k0:k1].max()
        quiet = np.nonzero(db[k0:k1] > max(peak_db - 35, floor + 3))[0]
        end = min(end, start + (quiet[-1] + 2) * hop)
        step = mono[start:end].copy()
        fade_in, fade_out = int(0.003 * RATE), min(int(0.06 * RATE), len(step) // 3)
        step[:fade_in] *= np.sin(np.linspace(0, math.pi / 2, fade_in)) ** 2
        step[-fade_out:] *= np.cos(np.linspace(0, math.pi / 2, fade_out)) ** 2
        step = step[:, None]
        step *= 10 ** ((STEP_LUFS - lufs(step)) / 20)
        step = limit(step, 10 ** (-3 / 20), circular=False)
        path = out_dir / f"footstep_grass_{i:02d}.wav"
        write_wav(path, step)
        write_meta(path, {"force_to_mono": False, "load_type": "DecompressOnLoad"})
        print(f"{path.name}: {len(step) / RATE * 1000:5.0f} ms, peak {20 * math.log10(np.abs(step).max()):5.1f} dBFS")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("sfx", type=Path)
    ap.add_argument("project", type=Path)
    a = ap.parse_args()
    audio = a.project / "Assets" / "Audio"
    loops(a.sfx, audio)
    footsteps(a.sfx, audio)


if __name__ == "__main__":
    main()
