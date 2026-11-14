# Themes and fonts

You can change how the editor looks: pick a light or dark theme, make your own colours, and
choose the fonts for the interface and for code. Everything is in **Edit ▸ Preferences ▸
Appearance** and takes effect at once, with no restart. Your choice is saved for you (not for
the project), so it follows you to every project you open.

## Choosing a theme

Open **Edit ▸ Preferences** and use the **Theme** list under **Appearance**. The built-in
themes are:

| Theme | What it looks like |
|---|---|
| Dark | The editor's standard look: grey panels and a blue selection. |
| Light | Light grey panels with dark text, for bright rooms. |
| High Contrast | Black panels, white text and a yellow accent: the strongest contrast. |
| Warm Dark | A dark theme with warm brown tones and an orange accent. |
| Midnight | A deep blue dark theme. |

The whole editor follows the theme: panels, menus, tabs, the Properties cards, the Console,
the graph editors and the Code Editor. Icons are tinted to stay readable.

> [!TIP]
> The Hub (the project list you see before a project opens) follows the theme too: a light
> theme makes the Hub light, a dark one makes it dark. Choosing Dark or Light in the Hub's
> Settings also sets the editor's theme.

## Making your own colours

Click **Theme Editor…** next to the Theme list. The Theme Editor shows every colour of the
current theme, in groups:

- **Panels**: the background, panels, tabs, toolbars, popups, cards, fields, buttons, hover
  and borders;
- **Text**: normal, weak, strong and selected text;
- **Accents**: the accent colour, the selection, prefabs and overrides;
- **Status**: warnings, errors and success;
- **Graphs**: the background, node body, node header and selection of the Script, Shader and
  Effect Graph editors and the Animator;
- **Code**: the Code Editor's background, text, line numbers, current line and selection.

Click a colour to change it, or type a colour code such as `#3a7bd5`. The editor changes as
you drag, so you see the result right away. A small sample at the top shows text, the
selection, the status colours and a graph node.

The built-in themes can't be changed directly: the first time you change a colour of one, a
copy is saved as a custom theme (for example "Dark Custom") and you edit that.

The buttons at the top manage your themes:

| Button | What it does |
|---|---|
| Duplicate | Copies the current theme to a new custom theme. |
| Rename | Gives the current custom theme a new name. |
| Delete | Deletes the current custom theme and goes back to the theme it started from. |
| Reset to Preset | Gives the current custom theme the colours of a built-in theme. |
| Import… | Adds a theme from a `.mistytheme` file. |
| Export… | Saves the current theme as a `.mistytheme` file to share. |

> [!NOTE]
> **Keep text readable.** When text and its background are too close in brightness, the Theme
> Editor shows a warning such as "Low contrast: Text on Panel (2.1:1)". Normal text should
> reach at least 4.5:1 and weak text 3:1.

> [!PROGRAMMERS]
> Custom themes are small JSON files in your user folder (`%APPDATA%\MistyEngine\themes` on
> Windows, `~/.config/MistyEngine/themes` on Linux). A file names the theme it starts from and
> lists its colours as `#rrggbb`; colours it leaves out come from that base theme:
>
> ```json
> { "format": 1, "name": "Paper", "base": "light",
>   "colors": { "accent": "#c05a00", "selection": "#ffd9b3" } }
> ```

## Choosing fonts

Under **Appearance** you can also set:

- **UI Font**: the font of the whole interface. Click it to open a list of the fonts
  installed on your computer, type in **Search** to find one, and click it. The sample line
  under the list shows Latin, Turkish and Arabic letters in the chosen font.
- **Code Font**: the font of the Code Editor and the Console. The list shows monospace fonts
  (every letter the same width); tick **All fonts** to see the others.
- **Text Size**: makes all text smaller or larger, from 80% to 150%.

**Default** next to a font goes back to the editor's own font.

> [!TIP]
> If a font has no Arabic, Turkish or symbol letters, the editor draws those letters with its
> built-in fonts, so nothing goes missing.

### When a font is missing

If the chosen font is no longer there when the editor starts (it was uninstalled, or you
opened your settings on another computer), the editor quietly uses its default font and shows
a small note in the bottom corner once. Pick another font in Preferences, or click
**Default**.
