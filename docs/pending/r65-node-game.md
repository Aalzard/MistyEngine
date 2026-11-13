# Round 65: a game made only with nodes

A short 2D platformer ("Lumo's Leap") built with Script Graphs only: no Lua, no Rust gameplay
code. The point is to prove the nodes a real game needs, and to list what is broken or missing.

## Issues

Each issue: how to reproduce, the graph, expected and actual. Status is updated when a fix is
merged.

### 1. 2D triggers and collisions never reach Script Graphs

- **Repro:** an object with a Rigidbody 2D, a Box Collider 2D and a graph with
  `On Trigger Enter -> Debug.Log("hit")` (or `On Collision Enter`). Move it into another 2D
  collider (trigger or solid).
- **Graph:** `OnTriggerEnter.Out -> Api(Debug.Log).In`, `OnTriggerEnter.Other -> Message`.
- **Expected:** the event runs with `Other` = the other object, as it does for 3D colliders (and as
  Lua gets `OnTriggerEnter2D` / `OnCollisionEnter2D`).
- **Actual:** nothing runs. `ScriptHost::dispatch_2d`
  (`crates/misty_engine/src/scripting/mod.rs`) calls only Lua instances; the 3D `dispatch` calls
  `self.graphs.contact(...)` first, the 2D one never does. A 2D game cannot use the four contact
  events at all.
- **Workaround used:** distance checks and `Physics2D.OverlapCircleAll` each frame.
- **Status:** reported.

### 2. A UI Button's On Click cannot run a Script Graph

- **Repro:** a Button whose On Click entry targets an object that has a graph with
  `Custom Event: Play`, method `Play`. Click the button while playing.
- **Expected:** the graph's `Custom Event: Play` runs (the entry is "call `method` on the scripts
  of `target`", and graphs are scripts; `gameObject:SendCustomEvent` already reaches both).
- **Actual:** a Console warning, `On Click: no script on 'Menu' has a method 'Play'`, and nothing
  runs. `ScriptHost::send_message_with` only looks at Lua instances; the same holds for On Value
  Changed and On End Edit of the other widgets. There is no node to listen to a button either
  (`button.onClick:AddListener` needs a Lua function).
- **Workaround used:** the menu reads keys and the mouse with `Input` nodes.
- **Status:** reported.

### 3. API nodes of variadic Lua functions have no input pins

- **Repro:** in the node search pick `Physics2D: Raycast`, `Rigidbody2D: AddForce`,
  `Rigidbody2D: MovePosition`, `Input: GetButton`.
- **Expected:** pins for the arguments (origin, direction, distance, layer mask…).
- **Actual:** the node has no inputs: the binding's signature is `(...)`, and
  `signature_args` (`crates/misty_editor/src/panels/script_graph/search.rs`) drops `...`. The
  node can only be used after typing names into Graph Properties ▸ Arguments, which nothing
  hints at.
- **Workaround used:** `args` typed by hand (`origin, direction, distance, mask`).
- **Status:** reported.
