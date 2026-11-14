"""Writes the node game ("Lumo's Leap") into a project: Script Graphs, scenes and a prefab
under Assets/NodeGame. The game is made only of Script Graph nodes; this script just saves
the graph and scene files, like the editor's Script Graph window and Save Scene do.

    python tools/node_game/node_game.py <project folder> [--demo]

`--demo` writes the stages with the Autopilot graph in place of Player Input, so the game
plays itself (used for the screenshots).
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import graphs  # noqa: E402
import scenes  # noqa: E402


def write(root, rel, text):
    path = os.path.join(root, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 1:
        sys.exit(__doc__)
    root = args[0]
    demo = "--demo" in sys.argv
    for name, build in graphs.ALL.items():
        write(root, f"{graphs.GRAPHS}/{name}.vgraph", build().to_json())
    write(root, f"{graphs.PREFABS}/Burst.prefab", scenes.burst_prefab().to_json())
    for name, scene in scenes.all_scenes(demo).items():
        write(root, f"Assets/NodeGame/Scenes/{name}.scene", scene.to_json())


if __name__ == "__main__":
    main()
