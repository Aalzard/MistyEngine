# Custom title bar and flat play buttons (branch ui-titlebar)

## Progress
- The window is frameless (`with_decorations(false)`, resizable, not transparent) for both the
  Hub and the editor; `title_bar.rs` draws the bar.
- Editor bar: menus on the left, the logo and "Misty Engine — <project> — <scene>" in the centre
  ("Misty Engine" bright, the rest dim, `*` while unsaved), flat minimise / maximise (restore) /
  close on the right (close turns red on hover). The taskbar title is still sent each change.
- Hub bar: same bar in the Hub's colours with "Misty Hub" centred.
- Toolbar: Play / Pause / Step are flat painted icons (no frame or outline, soft hover, accent
  icon and faint accent fill while active). The logo + "Misty" moved off the toolbar into the
  title bar. Layers / Layout dropdowns unchanged.

## Decisions
- **Hub frameless too.** The Hub and the editor share one window (launcher.rs switches between
  them), so one frameless window with the same bar in both is simpler than toggling decorations
  at hand-over, and looks consistent.
- **Own resize zones.** winit 0.30 on Windows extends the client area over the whole window for
  undecorated windows (WM_NCCALCSIZE) and has no WM_NCHITTEST handler, so the OS gives no resize
  border. `title_bar::resize_edges` adds 5 pt edge / 10 pt corner zones (small foreground Areas,
  so they win over panels under them) that send `ViewportCommand::BeginResize`; hidden while
  maximised or fullscreen. Min sizes are unchanged.
- **Move / maximise.** Dragging the empty bar sends `StartDrag` (OS move, so Aero Snap and
  drag-to-restore work); double-click toggles `Maximized`. The bar's background is registered
  before the menus and buttons so those keep their clicks.
- **Close** sends `ViewportCommand::Close`, which egui-winit turns into a `close_requested`
  event, so the existing unsaved-changes prompt in `scene_file_frame` still intercepts it.
- **Glyphs painted, not font icons**: the window buttons and play icons are drawn with the
  painter (pixel-snapped 1 px lines / filled shapes) so they are crisp at any scale and do not
  depend on font coverage.
- Button order stays Play, Pause, Step (the user's wording and the existing order).
- theme.rs untouched; the bar uses existing theme colours and local styling only.
