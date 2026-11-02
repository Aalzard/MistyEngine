# Misty Engine

A Rust game engine and editor with a familiar Unity-style workflow, built on wgpu, winit, egui, hecs, rapier and Lua.

## Run

```bash
cargo run -p misty_editor --release
```

The editor opens `SampleProject/` (or pass a project folder: `cargo run -p misty_editor --release -- path/to/Project`).
A project is any folder with an `Assets/` directory.

## What's inside

| Area | Highlights |
|---|---|
| Editor | Dockable Hierarchy / Scene / Game / Inspector / Project / Console, menus, shortcuts, undo/redo |
| Scene view | Fly/orbit/pan camera, scene gizmo, 2D mode, move/rotate/scale handles, draw modes |
| Rendering | Lit meshes, directional/point/spot lights with hard/soft shadows, materials, selection outline |
| Physics | Rigidbody, Box/Sphere/Capsule/Mesh colliders (rapier) |
| Scripting | Lua scripts with Start/Update/FixedUpdate/collision callbacks — see [docs/scripting.md](docs/scripting.md) |
| Scenes | Save/load `.scene` files, recent scenes, unsaved-changes prompt |

Docs: [progress](docs/progress.md) · [decisions](docs/decisions.md) · [behaviour notes](docs/unity-notes.md)

## Develop

```bash
cargo test
cargo clippy --all-targets
```

`MISTY_SCREENSHOT=out.png` makes the editor save a screenshot of its window and exit (used for visual checks).
