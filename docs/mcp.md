# AI tools (MCP)

Misty Engine speaks the [Model Context Protocol](https://modelcontextprotocol.io), so AI agents
such as Claude (Claude Desktop, Claude Code) and Codex can drive the editor: look at the scene,
create and change objects and components, write scripts and materials, run menu commands, press
Play, read the Console, take screenshots, run the project's tests and build the game.

There are two ways to connect, with the same tools:

| | `misty --mcp <project>` | `misty --mcp-connect <project>` |
|---|---|---|
| What it is | The project opened **without a window**, serving the tools over stdio | A stdio proxy to the **editor you have open** |
| Needs | Nothing running | The editor open on that project, with **Allow AI tools (MCP)** on |
| You see the edits | In the files (open the project afterwards) | Live, in the editor, with undo |
| Play / Stop / Pause | No | Yes |
| Destructive tools | Your MCP client asks you (they are marked destructive) | The editor asks you: "AI wants to …  Allow / Deny" |

Both are local: the stdio server has no network port at all, and the live bridge listens on
`127.0.0.1` only, behind a random token.

## Set up a client

Use the full path to the editor if it is not on your `PATH`: in an installed or downloaded
release it is `MistyEngine.exe` (`MistyEngine` on Linux); built from source it is `misty`
(`misty.exe`). Give it the project folder (the one with `Assets/` in it). For the live editor,
replace `--mcp` with `--mcp-connect`.

### Claude Code

```bash
claude mcp add misty -- misty --mcp "/path/to/MyGame"
```

or, to share it with the project, a `.mcp.json` at the project root:

```json
{
  "mcpServers": {
    "misty": {
      "command": "misty",
      "args": ["--mcp", "."]
    }
  }
}
```

### Claude Desktop

Settings ▸ Developer ▸ Edit Config opens `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "misty": {
      "command": "C:\\Program Files\\Misty Engine\\MistyEngine.exe",
      "args": ["--mcp", "C:\\Users\\me\\Misty Projects\\MyGame"]
    }
  }
}
```

Restart Claude Desktop; the tools show under the tools icon.

### Codex

```bash
codex mcp add misty -- misty --mcp /path/to/MyGame
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.misty]
command = "misty"
args = ["--mcp", "/path/to/MyGame"]
```

### Any other client

Start `misty --mcp <project>` (or `misty --mcp-connect <project>`) with stdin and stdout as pipes.
Messages are JSON-RPC 2.0, one per line, as the MCP stdio transport specifies. Logs go to stderr.
Protocol revisions 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05 are accepted.

## The live editor

1. Open the project in the editor.
2. **Edit ▸ Preferences ▸ AI Tools**: tick **Allow AI tools (MCP)**. It is off by default and is a
   per-user setting (`mcp.json` next to the Hub's `hub.json`).
3. The Console says `AI tools (MCP) listening on 127.0.0.1:<port>`, and Preferences shows the
   client command with a Copy button.
4. Point your client at `misty --mcp-connect "<project>"`.

How the connection works: the editor listens on a loopback port the OS picks and writes
`Library/MistyMcp.json` (`port`, `token`, `pid`, `version`) in the project. `--mcp-connect` reads
it, connects to `127.0.0.1:<port>`, sends `{"token": "<token>"}` as its first line, and after
`{"ok": true}` copies MCP messages between its stdio and the editor. A wrong token gets
`{"ok": false, …}` and the connection is closed before anything is read. Unticking the setting,
or closing the editor, stops listening and deletes the file.

In the editor, every request runs on the main thread between frames, exactly like a menu
command: scene edits are normal undo steps (Ctrl+Z takes back what the AI did), the Hierarchy and
Inspector update at once, and a toast says when a client connects or disconnects.

## Tools

Objects are named by **id** (`"#4294967297"`, from `scene.list_hierarchy`; stable while the scene
stays open), by **path** (`"Environment/Lamp"`) or by a **unique name**. Paths of files are
project-relative (`"Assets/Scripts/Player.lua"`). Every tool has a JSON schema for its arguments;
unknown or mistyped arguments are refused with the list of expected ones. Clients that only allow
`[A-Za-z0-9_-]` in names can call `scene_create_object` for `scene.create_object`.

### Scene

| Tool | What it does |
|---|---|
| `scene.list_hierarchy` | Open scenes (unsaved changes, active) and their objects as a tree; `filter` gives a flat list by name or `t:Component` / `t:Script` |
| `scene.get_object` | One object in full: tag, layer, parent, children, prefab, every component with its field values |
| `scene.create_object` | Create an empty object, primitive, light, camera, audio source, particle system… with name, parent, position, rotation, scale and extra components |
| `scene.instantiate_prefab` | Place a `.prefab` or a model (`.glb`, `.gltf`, `.obj`) |
| `scene.set_field` | Set a component field (`"intensity"`, a dotted path like `"color.0"`, `GameObject` name/active/tag/layer, Transform `euler`, a script's public field) |
| `scene.add_component` / `scene.remove_component` | By component name, or a Lua script by path |
| `scene.set_parent` | Re-parent (keeps the world position), or move to the root |
| `scene.delete_objects` | Delete objects and their children |
| `scene.select` | Set the Hierarchy selection |
| `scene.save` | Save changed scenes (the prefab in Prefab Mode); `path` for a scene with no file yet |
| `scene.open` | Open a `.scene` (or `additive`); refuses over unsaved changes unless `discard_changes` |
| `scene.component_types` | The component names `add_component` accepts |

### Assets

| Tool | What it does |
|---|---|
| `asset.search` | Files under `Assets/` by name words and kind (texture, material, script, model, audio, scene, prefab, extension) |
| `asset.read_text` | A text file of the project (scripts, scenes, materials, settings), up to 256 KB |
| `asset.create_script` | A Lua script (the behaviour template by default); reports syntax errors; `overwrite` replaces a file |
| `asset.create_material` | A `.mat` with a color and any other material fields |
| `asset.import` | Copy a file from elsewhere on the computer into an `Assets/` folder (never overwrites) |
| `asset.delete` | Move an asset to the Recycle Bin / Trash |

### Editor

| Tool | What it does |
|---|---|
| `editor.get_state` | Project, scenes, Play mode, selection, undo/redo, Console counts |
| `editor.list_menu_items` / `editor.run_menu_item` | Any menu bar command by path (`"GameObject/3D Object/Cube"`, `"Assets/Refresh"`, editor-script menu items); returns what it logged |
| `editor.play` / `editor.stop` / `editor.pause` | Play mode (live editor only) |
| `editor.read_console` | Console lines with an index; `since` reads only new ones, `level` filters |
| `editor.screenshot` | A PNG of the Scene view or the Game view, as MCP image content |
| `editor.run_tests` | The project's Lua tests (EditMode / PlayMode, `filter`), with failures |
| `editor.build` | Build the player into a folder; returns the executable's path |
| `editor.undo` / `editor.redo` | Undo or redo steps, the AI's or anyone's |

In `--mcp` mode the Scene view screenshot uses the camera position saved with the project, and the
renderer starts on the first screenshot (a GPU is needed; the tool says so when there is none).
In the live editor a screenshot without a size is the view as it is on screen (without the editor
overlays); with a size, or for a closed view, it is rendered offscreen.

## Security

- **Local only.** `--mcp` has no network port. The live bridge binds `127.0.0.1` (never
  `0.0.0.0`), so other computers cannot reach it, and it is off until you tick the setting.
- **Token.** Each start of the bridge makes a new 256-bit random token, stored in the project's
  `Library/MistyMcp.json` (owner-only permissions on macOS and Linux). Any program running as you
  can read your project folder, and therefore the token: treat "Allow AI tools" like leaving the
  editor unlocked for your own programs. Keep `Library/` out of version control (it is per-user
  state anyway).
- **Confirmation.** In the live editor, deleting an asset, overwriting a file, opening a scene
  over unsaved changes, building, and menu commands that build, wait for **Allow / Deny** in the
  editor; nobody answering for two minutes means Deny. Commands that would close or restart the
  editor are refused outright. With `--mcp` there is no editor window to ask in, so the MCP client
  asks instead (those tools carry `destructiveHint`).
- **Contained writes.** File tools write only under `Assets/` and refuse `..` and absolute paths;
  `asset.read_text` reads inside the project only; `asset.import` copies in, never out, and never
  overwrites. Builds go to the folder you name, outside `Assets/`.
- **Undo.** Scene edits are ordinary undo steps. File writes are not undoable, which is why
  overwriting and deleting ask first (deleted assets go to the Recycle Bin / Trash).

## Notes and limits

- `--mcp` handles one request at a time; a test run or a build answers when it is done.
- In `--mcp` mode, menu commands that only change the window (layouts, focusing a tab) do nothing,
  and commands that open a file dialog are refused with the tool to use instead.
- Object ids change when a scene is reopened; use paths or names across reloads.
- The live bridge serves any number of clients at once, all on the editor's main thread.
