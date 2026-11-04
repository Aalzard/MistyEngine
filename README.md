# Misty Engine

A Rust game engine and editor with a familiar Unity-style workflow, built on wgpu, winit, egui, hecs, rapier and Lua.

## Run

```bash
cargo run -p misty_editor --release
```

This opens **Misty Hub**: recent projects, New Project (Empty, Standard, Ocean or Landscape template) and Open.
Pass a project folder to go straight to the editor (`cargo run -p misty_editor --release -- SampleProject`), or
`-- --new Landscape path/to/MyGame` to create a project from a template and open it.
A project is any folder with an `Assets/` directory.

### Play a build

```bash
cargo build -p misty_player --release
```

Build the player next to the editor first, then in the editor use **File ▸ Build Settings…** to list the scenes
(scene 0 starts the game) and press **Build** or **Build And Run** (Ctrl+B). The output folder holds
`<Product Name>.exe` with copies of `Assets/` and `ProjectSettings/`; run the exe to play.
To play a project without building: `cargo run -p misty_player --release -- path/to/Project`
(no argument = `SampleProject/`).

## What's inside

| Area | Highlights |
|---|---|
| Editor | Dockable Hierarchy / Scene / Game / Inspector / Project / Console, menus, shortcuts, undo/redo |
| Scene view | Fly/orbit/pan camera, scene gizmo, 2D mode, move/rotate/scale handles, draw modes |
| Rendering | Lit meshes, directional/point/spot lights with hard/soft shadows, materials, selection outline |
| Physics | Rigidbody, Box/Sphere/Capsule/Mesh colliders (rapier) |
| Scripting | Lua scripts with Start/Update/FixedUpdate/collision callbacks — see [docs/scripting.md](docs/scripting.md) |
| Scenes | Save/load `.scene` files, recent scenes, unsaved-changes prompt |
| Player | Standalone `misty_player` window, Build Settings scene list, Build / Build And Run |

Docs: [progress](docs/progress.md) · [decisions](docs/decisions.md) · [behaviour notes](docs/unity-notes.md)

## Develop

```bash
cargo test
cargo clippy --all-targets
```

`MISTY_SCREENSHOT=out.png` makes the editor (or `misty_player`) save a screenshot of its window and exit (used for
visual checks).
