# Built-in fonts

`NotoSansArabic-Subset.ttf`: Noto Sans Arabic Regular 2.013 (The Noto Project Authors,
https://github.com/notofonts/arabic), under the SIL Open Font License 1.1 (`OFL.txt`).

Modified for Misty Engine (the license allows it; Noto has no Reserved Font Name):
- subset with fontTools to U+0600–06FF, U+0750–077F, U+08A0–08FF, U+200C–200F, U+FB50–FBFF,
  U+FDF2, U+FDFC, U+FE70–FEFF and U+25CC, keeping every layout feature (shaping) and dropping
  hinting (68 KB instead of 142 KB);
- line metrics set to Segoe UI's proportions (ascent 1079, descent −251, no line gap, per 1000
  units in hhea, OS/2 typo and win metrics), so the editor's rows keep their height when this
  font is first in egui's font list.

```
pyftsubset NotoSansArabic-Regular.ttf --unicodes="U+0600-06FF,U+0750-077F,U+08A0-08FF,U+200C-200F,U+FB50-FBFF,U+FDF2,U+FDFC,U+FE70-FEFF,U+25CC" --layout-features='*' --name-IDs='*' --name-languages='*' --no-hinting --output-file=NotoSansArabic-Subset.ttf
```
Used by games (a fallback of every UI Text) and by the editor (Arabic UI).
