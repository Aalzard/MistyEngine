"""Round 47: before / after strip around the biggest pop of a dolly block.
Usage: python lod_dolly_strip.py <dir> <block x> <block y> <out.jpg> [crop w=256] [crop h=144]
Top row the instant switch, bottom row the cross-fade, frames around the jump, 2x."""
import sys, glob, numpy as np
from PIL import Image, ImageDraw
d, x, y, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
W, H = int(sys.argv[5]) if len(sys.argv) > 5 else 256, int(sys.argv[6]) if len(sys.argv) > 6 else 144
B = 32
def series(tag):
    s = []
    for f in sorted(glob.glob(f"{d}/{tag}_*.png")):
        a = np.asarray(Image.open(f).convert("RGB"), dtype=np.float32).mean(axis=2)
        s.append(a[y:y+B, x:x+B].mean())
    return np.array(s)
so, sn = series("off"), series("on")
t = int(np.abs(np.diff(so)).argmax())  # off jumps between t and t+1
frames = list(range(max(0, t - 2), min(len(so), t + 4)))
print("jump between frames", t, t + 1, "off steps", np.round(np.diff(so)[max(0,t-3):t+4], 1), "on steps", np.round(np.diff(sn)[max(0,t-3):t+4], 1))
cx, cy = max(0, x + B // 2 - W // 2), max(0, y + B // 2 - H // 2)
S = 2
sheet = Image.new("RGB", (len(frames) * W * S, 2 * H * S + 24), "white")
dr = ImageDraw.Draw(sheet)
for row, tag in enumerate(("off", "on")):
    for col, f in enumerate(frames):
        im = Image.open(f"{d}/{tag}_{f:03d}.png").convert("RGB").crop((cx, cy, cx + W, cy + H)).resize((W * S, H * S), Image.NEAREST)
        sheet.paste(im, (col * W * S, 24 + row * H * S))
        dr.text((col * W * S + 4, 24 + row * H * S + 4), f"{tag} frame {f}", fill="yellow")
dr.text((4, 4), f"top: fades off (instant switch), bottom: cross-fade; crop {W}x{H} at ({cx},{cy}), x{S}", fill="black")
sheet.save(out, quality=92)
