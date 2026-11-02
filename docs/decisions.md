# Decisions

Record of choices made without asking, and why.

## 2026-11-03 — Step 1

1. **Unity not installed → docs-based notes.** Only an empty `Unity\Hub` folder exists. `docs/unity-notes.md`
   is written from the official Unity Manual instead of a live editor.
2. **Engine name: "Misty Engine".** Matches the project folder and avoids using Unity's name.
3. **eframe as the app shell.** eframe already wraps winit + wgpu + egui (`wgpu` renderer feature), so we get all
   three requested libraries without hand-writing the glue. The Scene/Game views will render 3D through
   `egui_wgpu` paint callbacks into offscreen textures, which keeps everything in one wgpu device.
4. **egui 0.36 + egui_dock 0.21.** egui_dock is the most mature Unity-style docking crate. Its latest version
   targets egui 0.36, so we pin eframe to 0.36 instead of 0.37.
5. **Cargo workspace (`crates/*`).** Editor binary now; an engine library (ECS, renderer, physics) gets its own
   crate in later steps so games can run without the editor.
6. **Placeholder scene data in step 1.** The Hierarchy and Inspector use a small in-memory list of objects so
   the panels can be laid out and clicked. It is replaced by the ECS in step 3.
7. **Transform tools stay in the main toolbar** (older Unity layout) instead of a Scene-view overlay. It is simpler
   and always visible; an overlay can come later.
8. **Sample project folder.** The editor opens `SampleProject/` by default (or a path passed on the command
   line) and the Project window lists the real files under `SampleProject/Assets`.
