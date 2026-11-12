"""Round 47: pops in a LOD fade dolly (frames from `lod_fade_dolly` with MISTY_DOLLY_DIR).
Usage: python lod_dolly_pops.py <dir> [block=32]
Per 32 px block, the off (instant) and on (cross-fade) runs: the blocks where they differ most
(trees inside a band) with their worst one-frame step and second difference, then whole-frame
counts of blocks whose second difference exceeds 6 and 12 (of 255)."""
import sys, glob, numpy as np
from PIL import Image
d = sys.argv[1]; B = int(sys.argv[2]) if len(sys.argv) > 2 else 32
def load(tag):
    m = []
    for f in sorted(glob.glob(f"{d}/{tag}_*.png")):
        a = np.asarray(Image.open(f).convert("RGB"), dtype=np.float32).mean(axis=2)
        h, w = a.shape[0] // B * B, a.shape[1] // B * B
        m.append(a[:h, :w].reshape(h // B, B, w // B, B).mean(axis=(1, 3)))
    return np.stack(m)
off, on = load("off"), load("on")
# Blocks where the two runs differ most at some frame: a tree inside a band.
diff = np.abs(off - on).max(axis=0)
idx = np.argsort(diff.ravel())[::-1][:8]
for k in idx:
    y, x = np.unravel_index(k, diff.shape)
    so, sn = off[:, y, x], on[:, y, x]
    # Steps between frames beyond the block's smooth trend (second difference).
    d2o = np.abs(so[:-2] - 2 * so[1:-1] + so[2:]).max()
    d2n = np.abs(sn[:-2] - 2 * sn[1:-1] + sn[2:]).max()
    print(f"block ({x*B},{y*B}) runs differ by {diff[y,x]:.1f}: worst 2nd diff off {d2o:.1f} on {d2n:.1f}; max step off {np.abs(np.diff(so)).max():.1f} on {np.abs(np.diff(sn)).max():.1f}")
d2 = lambda s: np.abs(s[:-2] - 2 * s[1:-1] + s[2:])
print("whole frame, blocks with 2nd diff > 6: off", int((d2(off) > 6).sum()), "on", int((d2(on) > 6).sum()))
print("whole frame, blocks with 2nd diff > 12: off", int((d2(off) > 12).sum()), "on", int((d2(on) > 12).sum()))
