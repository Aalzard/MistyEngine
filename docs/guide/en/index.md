# Misty Engine User Guide

Welcome! This guide shows you how to use Misty Engine, from the first look at the editor to
building game logic with Script Graphs. You can read it right inside the engine
(**Help ▸ User Guide**) or as plain files in `docs/guide/`.

You don't need to know how to program. Where a little extra depth helps people who do, you'll
see a box like this:

> [!PROGRAMMERS]
> Notes for programmers: what happens under the hood, the Lua equivalent of a node, and so on.
> You can skip these boxes without missing anything you need.

## Chapters

1. [The editor layout](01-layout.md): the Scene Tree, Properties, Asset Files, the Editor
   Scene and the Scene Viewer, the Console, the toolbar, Play / Pause / Step, the menus,
   layouts and Preferences.
2. [Finding what is slow](02-performance.md): the Profiler, the Frame Debugger, the Stats
   box and the build report, with a worked example.
3. [Quality and rendering](03-quality-and-rendering.md): quality levels, Misty Mesh, Misty
   GI, LODs and occlusion culling: what each setting does and when to use it.
4. [Script Graphs: the basics](04-script-graphs-basics.md): events, wires, variables and
   the most useful nodes.
5. [Guide: a first-person player](05-guide-first-person.md): walk with WASD, look with the
   mouse, jump.
6. [Guide: a door that opens and closes](06-guide-door.md): a trigger zone, a key press,
   smooth turning and an open / closed state.
7. [Guide: saving and loading](07-guide-saving.md): keep the score and the player's
   position between runs.
8. [Guide: animation transitions](08-guide-animation.md): Idle, Walk, Run and Jump driven
   by speed through the Animator.

[Glossary](glossary.md): the words this guide uses, in one place.

## Try the examples

Every guide comes with a working example in the sample project, under `Assets/Guides/`.
In the in-engine guide, the **Open example** button at the top of a guide opens its scene and
its Script Graph. Press **Play** and try it, then change things and see what happens.

> [!TIP]
> New to the editor? Take the short interactive tour first: **Help ▸ Layout Tour**. It points
> at each part of the screen and says what it is for, in about two minutes.
