# Glossary

**Action node** — a Script Graph node with white flow pins that does something (Set a
variable, Move, Log).

**Animator / Animator Controller** — the Animator archetype plays the animations of an
object; its controller is the map of states, parameters and transitions.

**Archetype** — one ability of an object: Transform, Mesh Renderer, Light, Collider, Script
Graph… Added with **Add Archetype** in Properties.

**Asset** — a file of your project: a scene, model, texture, material, sound, script or graph.

**Asset Files** — the panel listing your project's files.

**Blackboard** — the left side of the Script Graph window, listing the graph's variables.

**Character Controller** — an archetype that moves a capsule through the world, sliding along
walls, without physics pushing it around.

**Collider / Trigger** — a collider gives an object a physical shape. With **Is Trigger** it
blocks nothing and only reports what enters and leaves it.

**Console** — the panel with messages, warnings and errors.

**Data wire** — a coloured wire that carries a value between nodes.

**Delta Time** — the time since the previous frame, in seconds. Multiply speeds by it so
things move at the same pace on every machine.

**Draw call (batch)** — one request to the GPU to draw something. Fewer is faster.

**Editor Scene** — the panel where you build the scene with a free camera.

**Event** — a node that starts a flow: On Start, On Update, On Trigger Enter…

**Flow wire (exec wire)** — a white wire that sets the order in which action nodes run.

**Frame** — one picture of the game. At 60 FPS a frame has about 16 ms.

**Frame Debugger** — replays one frame draw by draw.

**LOD (level of detail)** — simpler versions of a model used when it is small on screen.

**Misty GI** — Misty Engine's live global illumination: bounce light, sky shadowing and soft
reflections without baking.

**Misty Mesh** — Misty Engine's virtual geometry: dense models cut into clusters, drawn at the
detail the screen needs.

**Node** — one box of a Script Graph.

**Object** — a thing in a scene (a GameObject). Holds archetypes and child objects.

**Object variable** — a Script Graph variable stored on the object, editable per object in
Properties.

**Occlusion culling** — skipping objects hidden behind others.

**Pin** — a connection point on a node: inputs on the left, outputs on the right.

**Play mode** — running the game inside the editor (Play, Ctrl+P).

**PlayerPrefs** — small named values the game keeps between runs.

**Prefab** — a saved object (with its children and archetypes) you can place many times.

**Profiler** — measures where each frame's time goes.

**Properties** — the panel showing the selected object's archetypes and settings.

**Quality level** — a named set of rendering settings (Low, Medium, High, Ultra).

**Scene** — one level or screen of your game, saved as a `.scene` file.

**Scene Tree** — the panel listing the objects of the scene as a tree.

**Scene Viewer** — the panel showing the game through its camera; where Play runs.

**Script Graph** — game logic made of nodes and wires (a `.vgraph` file).

**Sequence** — a flow node that runs its outputs in order.

**Tag** — a label on an object (Player, Enemy) that logic can check with CompareTag.

**Target** — the object an engine node works on. Unconnected, it is the graph's own object.

**Transition** — an Animator arrow from one state to another, with its conditions.

**Trigger (Animator parameter)** — a one-shot signal that turns itself off once used.

**Value node** — a node without flow pins that only computes a value (Add, Get Variable).
