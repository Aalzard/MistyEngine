"""The scenes and prefabs of the node game, in the editor's scene file format: a menu, three
stages drawn as text maps, and a win screen. Objects only carry components and Script Graphs."""

import json

import graphs
from graphs import GRAPHS, PREFABS, SCENES

PLAYER_PNG = "Assets/Sprites/Player.png"
GROUND_TILE = "Assets/Tiles/Ground.tile"
SQUARE = "Builtin/Square.png"
CIRCLE = "Builtin/Circle.png"
TRIANGLE = "Builtin/Triangle.png"
IGNORE_RAYCAST = 2
UI_LAYER = 4
PLAYER_LAYER = 5
SKY = [0.42, 0.72, 0.95, 1.0]
ORTHO = 5.5
ASPECT = 1100.0 / 650.0


class Scene:
    def __init__(self):
        self.objects = []

    def add(self, name, parent=None, pos=(0.0, 0.0, 0.0), scale=(1.0, 1.0, 1.0), tag="Untagged",
            layer=0, static=False, **components):
        comps = {
            "Transform": {
                "euler_hint": [0.0, 0.0, 0.0],
                "position": [float(v) for v in pos],
                "rotation": [0.0, 0.0, 0.0, 1.0],
                "scale": [float(v) for v in scale],
            }
        }
        order = ["Transform"]
        for key, value in components.items():
            comps[key] = value
            order.append(key)
        comps["GameObject"] = {
            "active": True,
            "component_order": order,
            "is_static": static,
            "layer": layer,
            "name": name,
            "tag": tag,
        }
        self.objects.append({"components": comps, "parent": parent})
        return len(self.objects) - 1

    def comp(self, index, name):
        return self.objects[index]["components"][name]

    def to_json(self):
        return json.dumps({"objects": self.objects, "version": 1}, indent=2) + "\n"


def sprite(path, color=(1.0, 1.0, 1.0, 1.0), order=0):
    return {
        "color": list(color),
        "draw_mode": "Simple",
        "enabled": True,
        "flip_x": False,
        "flip_y": False,
        "order_in_layer": order,
        "sorting_layer": "Default",
        "sprite": path,
    }


def scripts(*entries):
    """Script Graphs on an object: names, or (name, {Object variable: value})."""
    out = []
    for e in entries:
        name, fields = (e, {}) if isinstance(e, str) else e
        out.append({"enabled": True, "fields": fields, "path": f"{GRAPHS}/{name}.vgraph"})
    return {"list": out}


def box(size, trigger=False, friction=0.4):
    return {
        "edge_radius": 0.0,
        "enabled": True,
        "is_trigger": trigger,
        "material": {"bounciness": 0.0, "friction": friction},
        "offset": [0.0, 0.0],
        "size": list(size),
    }


def body(kind="Dynamic", gravity=3.0):
    return {
        "angular_drag": 0.05,
        "body_type": kind,
        "collision_detection": "Discrete",
        "freeze_position": [False, False],
        "freeze_rotation": True,
        "gravity_scale": gravity,
        "linear_drag": 0.0,
        "mass": 1.0,
    }


def rect(anchor_min, anchor_max, pivot, pos, size):
    return {
        "anchor_min": list(anchor_min),
        "anchor_max": list(anchor_max),
        "anchored_position": list(pos),
        "pivot": list(pivot),
        "size_delta": list(size),
    }


def text(value, size, color=(1.0, 1.0, 1.0, 1.0), align="MiddleCenter"):
    return {
        "alignment": align,
        "color": list(color),
        "enabled": True,
        "font_size": float(size),
        "line_spacing": 1.0,
        "raycast_target": False,
        "text": value,
        "wrap": False,
    }


def mm(a, b=None):
    return {"a": a, "b": a if b is None else b, "random": b is not None}


def particles(*, rate=0.0, burst=0, lifetime=(0.25, 0.45), speed=(0.2, 0.8), size=(0.12, 0.3),
              color=(1.0, 1.0, 1.0, 1.0), looping=True, awake=False, radius=0.3, gravity=0.0,
              intensity=2.5):
    bursts = []
    if burst:
        bursts.append(
            {"time": 0.0, "count": burst, "cycles": 1, "interval": 0.01, "probability": 1.0}
        )
    return {
        "color_over_lifetime": {
            "enabled": True,
            "gradient": {
                "alpha_keys": [{"alpha": 1.0, "time": 0.0}, {"alpha": 0.0, "time": 1.0}],
                "color_keys": [
                    {"color": [1.0, 1.0, 1.0], "time": 0.0},
                    {"color": [1.0, 1.0, 1.0], "time": 1.0},
                ],
            },
        },
        "emission": {"bursts": bursts, "enabled": True, "rate_over_time": mm(rate)},
        "main": {
            "duration": 1.0,
            "gravity_modifier": gravity,
            "looping": looping,
            "max_particles": 200,
            "play_on_awake": awake,
            "prewarm": False,
            "simulation_space": "World",
            "start_color": {"a": list(color), "b": list(color), "random": False},
            "start_delay": mm(0.0),
            "start_lifetime": mm(*lifetime),
            "start_size": mm(*size),
            "start_speed": mm(*speed),
        },
        "renderer": {"blend": "Additive", "enabled": True, "intensity": intensity, "texture": None},
        "seed": 0,
        "shape": {
            "angle": 25.0,
            "box_size": [1.0, 1.0, 1.0],
            "enabled": True,
            "radius": radius,
            "shape": "Sphere",
        },
        "size_over_lifetime": {"enabled": True, "end": 0.1, "start": 1.0},
    }


def camera(s, x, y, follow=None):
    comps = {
        "Camera": {
            "background": SKY,
            "clear_flags": "SolidColor",
            "depth": 0.0,
            "enabled": True,
            "far": 1000.0,
            "fov": 60.0,
            "near": 0.3,
            "ortho_size": ORTHO,
            "orthographic": True,
            "viewport": [0.0, 0.0, 1.0, 1.0],
        },
        "Audio Listener": {"enabled": True},
    }
    if follow is not None:
        comps["Scripts"] = scripts(("Camera Follow", follow))
    return s.add("Main Camera", pos=(x, y, -10.0), tag="MainCamera", **comps)


def light_and_glow(s):
    s.add(
        "Global Light 2D",
        **{
            "Light 2D": {
                "color": [1.0, 0.97, 0.92],
                "enabled": True,
                "falloff_strength": 0.5,
                "inner_angle": 360.0,
                "inner_radius": 0.0,
                "intensity": 1.0,
                "kind": "Global",
                "outer_angle": 360.0,
                "outer_radius": 5.0,
                "shadow_softness": 0.3,
                "shadow_strength": 1.0,
                "shadows_enabled": False,
                "target_sorting_layers": None,
            }
        },
    )
    s.add(
        "Post-process Volume",
        **{
            "Post-process Volume": {
                "bloom": {
                    "active": True,
                    "intensity": 0.6,
                    "scatter": 0.7,
                    "threshold": 1.0,
                    "tint": [1.0, 1.0, 1.0],
                },
                "color_adjustments": {
                    "active": False,
                    "color_filter": [1.0, 1.0, 1.0],
                    "contrast": 0.0,
                    "post_exposure": 0.0,
                    "saturation": 0.0,
                },
                "enabled": True,
                "priority": 0.0,
                "tonemapping": {"active": False, "mode": "Aces"},
                "vignette": {
                    "active": True,
                    "color": [0.0, 0.0, 0.0],
                    "intensity": 0.15,
                    "smoothness": 0.2,
                },
            }
        },
    )


def backdrop(s, width, hill=(0.5, 0.76, 0.46, 1.0), far=(0.62, 0.84, 0.58, 1.0)):
    root = s.add("Backdrop")
    s.add("Sun", root, pos=(width * 0.5, 8.5, 6.0), scale=(2.4, 2.4, 1.0),
          **{"Sprite Renderer": sprite(CIRCLE, (2.6, 2.3, 1.4, 1.0), -30)})
    i = 0
    x = -12.0
    while x < width + 14.0:
        i += 1
        s.add(f"Far Hill {i}", root, pos=(x, -1.0, 5.0), scale=(16.0, 11.0, 1.0),
              **{"Sprite Renderer": sprite(CIRCLE, far, -26)})
        s.add(f"Hill {i}", root, pos=(x + 6.5, -2.0, 4.0), scale=(13.0, 9.0, 1.0),
              **{"Sprite Renderer": sprite(CIRCLE, hill, -25)})
        x += 13.0
    return root


def canvas(s):
    c = s.add(
        "Canvas",
        layer=UI_LAYER,
        **{
            "Rect Transform": rect((0, 0), (1, 1), (0.5, 0.5), (0, 0), (0, 0)),
            "Canvas": {"enabled": True, "render_mode": "ScreenSpaceOverlay", "sort_order": 0},
            "Canvas Scaler": {
                "enabled": True,
                "match_width_or_height": 0.5,
                "mode": "ScaleWithScreenSize",
                "reference_resolution": [1600.0, 900.0],
                "scale_factor": 1.0,
            },
        },
    )
    return c


def label(s, parent, name, value, size, anchor, pos, box_size, color=(1.0, 1.0, 1.0, 1.0),
          align="MiddleCenter"):
    return s.add(
        name,
        parent,
        layer=UI_LAYER,
        **{
            "Rect Transform": rect(anchor, anchor, anchor, pos, box_size),
            "Text": text(value, size, color, align),
        },
    )


def player(s, x, y, driver):
    """`driver`: the graph that feeds the Player graph (Player Input, or Autopilot in the
    demo stages)."""
    return s.add(
        "Player",
        pos=(x, y, 0.0),
        tag="Player",
        layer=PLAYER_LAYER,
        **{
            "Sprite Renderer": sprite(PLAYER_PNG, order=5),
            "Rigidbody 2D": body(),
            "Box Collider 2D": box((0.7, 0.95), friction=0.0),
            "Particle System": particles(rate=70.0, lifetime=(0.2, 0.35), size=(0.2, 0.45)),
            "Scripts": scripts("Player", driver),
        },
    )


def eyes(s, parent, color=(1.0, 1.0, 1.0, 1.0)):
    for i, dx in enumerate((-0.2, 0.2)):
        e = s.add(f"Eye {i + 1}", parent, pos=(dx, 0.12, 0.0), scale=(0.2, 0.26, 1.0),
                  **{"Sprite Renderer": sprite(SQUARE, color, 7)})
        s.add("Pupil", e, pos=(-0.15, -0.1, 0.0), scale=(0.5, 0.5, 1.0),
              **{"Sprite Renderer": sprite(SQUARE, (0.08, 0.08, 0.12, 1.0), 8)})


def brute(s, parent, i, x, y):
    e = s.add(
        f"Brute {i}",
        parent,
        pos=(x, y, 0.0),
        scale=(0.95, 0.95, 1.0),
        layer=IGNORE_RAYCAST,
        **{
            "Sprite Renderer": sprite(SQUARE, (0.78, 0.22, 0.2, 1.0), 4),
            "Rigidbody 2D": body("Kinematic", 0.0),
            "Box Collider 2D": box((1.0, 1.0), trigger=True),
            "Scripts": scripts(("Patrol", {"Speed": 1.4}), "Enemy Touch", "Brute"),
        },
    )
    eyes(s, e)
    return e


def puff(s, parent, i, x, y):
    e = s.add(
        f"Puff {i}",
        parent,
        pos=(x, y - 0.1, 0.0),
        scale=(0.8, 0.8, 1.0),
        layer=IGNORE_RAYCAST,
        **{
            "Sprite Renderer": sprite(CIRCLE, (0.75, 0.6, 1.0, 1.0), 4),
            "Rigidbody 2D": body("Kinematic", 0.0),
            "Box Collider 2D": box((1.0, 1.0), trigger=True),
            "Scripts": scripts(("Patrol", {"Speed": 1.0}), "Enemy Touch", "Puff"),
        },
    )
    eyes(s, e)
    return e


def goal(s, x, y):
    g = s.add(
        "Goal",
        pos=(x, y, 0.0),
        layer=IGNORE_RAYCAST,
        **{
            "Box Collider 2D": box((1.0, 3.0), trigger=True),
            "Scripts": scripts("Goal"),
        },
    )
    s.add("Pole", g, pos=(0.0, 0.5, 0.0), scale=(0.12, 3.0, 1.0),
          **{"Sprite Renderer": sprite(SQUARE, (0.92, 0.92, 0.95, 1.0), 2)})
    s.add("Flag", g, pos=(0.45, 1.55, 0.0), scale=(0.8, 0.7, 1.0),
          **{"Sprite Renderer": sprite(TRIANGLE, (2.2, 1.7, 0.4, 1.0), 3)})
    return g


def stage(index, rows, driver="Player Input"):
    """A stage from a text map (top row first): `#` ground, `P` player, `B` Brute, `U` Puff,
    `G` goal. One character is one tile; the bottom row is y = -2."""
    s = Scene()
    width = max(len(r) for r in rows)
    height = len(rows)
    cells, spawn = [], {}
    for r, row in enumerate(rows):
        y = height - 1 - r - 2
        for x, ch in enumerate(row):
            if ch == "#":
                cells.append([x, y, 0, 0])
            elif ch in "PBUG":
                spawn.setdefault(ch, []).append((x + 0.5, y + 0.5))
    # Fill under the bottom row so the ground looks deep.
    for x, ch in enumerate(rows[-1]):
        if ch == "#":
            cells.extend([x, y, 0, 0] for y in (-3, -4, -5))
    cells.sort()
    half_w = ORTHO * ASPECT
    cam_min, cam_max = half_w, max(half_w, width - half_w)
    px, py = spawn["P"][0]
    camera(
        s,
        min(max(px, cam_min), cam_max),
        3.5,
        {"Min X": cam_min, "Max X": cam_max, "Height": 3.5},
    )
    light_and_glow(s)
    backdrop(s, width)
    grid = s.add("Grid", **{"Grid": {"cell_gap": [0.0, 0.0], "cell_size": [1.0, 1.0],
                                      "layout": "Rectangle"}})
    s.add(
        "Ground",
        grid,
        static=True,
        **{
            "Tilemap": {
                "animation_frame_rate": 1.0,
                "color": [1.0, 1.0, 1.0, 1.0],
                "tile_anchor": [0.5, 0.5],
                "tiles": {"cells": cells, "palette": [GROUND_TILE]},
            },
            "Tilemap Renderer": {
                "enabled": True,
                "mode": "Chunk",
                "order_in_layer": 0,
                "sorting_layer": "Default",
            },
            "Tilemap Collider 2D": {
                "common": {
                    "enabled": True,
                    "is_trigger": False,
                    # No friction: with the default 0.4 a player pushing against the side of
                    # a platform sticks to it (frictions are averaged).
                    "material": {"bounciness": 0.0, "friction": 0.0},
                    "offset": [0.0, 0.0],
                },
                "used_by_composite": True,
            },
        },
    )
    # Walls just outside the map keep everyone in.
    for name, x in (("Left Wall", -0.5), ("Right Wall", width + 0.5)):
        s.add(name, pos=(x, 6.0, 0.0), static=True, **{"Box Collider 2D": box((1.0, 30.0), friction=0.0)})
    enemies = s.add("Enemies")
    for i, (x, y) in enumerate(spawn.get("B", [])):
        brute(s, enemies, i + 1, x, y)
    for i, (x, y) in enumerate(spawn.get("U", [])):
        puff(s, enemies, i + 1, x, y)
    gx, gy = spawn["G"][0]
    goal(s, gx, gy + 0.5)
    player(s, px, py, driver)
    key = f"stage{index}"
    nxt = f"stage{index + 1}" if index < 3 else "win"
    s.add("Game", **{"Scripts": scripts(("Stage", {"This Scene": SCENES[key],
                                                    "Next Scene": SCENES[nxt]}))})
    c = canvas(s)
    dark = (0.08, 0.1, 0.2, 1.0)
    label(s, c, "Hearts Text", "Hearts: 3", 40, (0.0, 1.0), (28.0, -20.0), (400.0, 56.0), dark,
          "UpperLeft")
    label(s, c, "Dash Text", "Dash: ready", 30, (0.0, 1.0), (28.0, -72.0), (400.0, 44.0), dark,
          "UpperLeft")
    label(s, c, "Stage Text", f"Stage {index}", 40, (1.0, 1.0), (-28.0, -20.0), (400.0, 56.0),
          dark, "UpperRight")
    label(s, c, "Banner Text", "", 96, (0.5, 0.6), (0.0, 0.0), (1200.0, 140.0),
          (1.0, 0.97, 0.85, 1.0))
    label(s, c, "Controls Text", "A/D or arrows: run    Space: jump    E: dash attack", 24,
          (0.0, 0.0), (28.0, 20.0), (900.0, 36.0), (1.0, 0.97, 0.9, 0.9), "LowerLeft")
    return s


def button(s, parent, name, caption, y, target, method):
    b = s.add(
        name,
        parent,
        layer=UI_LAYER,
        **{
            "Rect Transform": rect((0.5, 0.5), (0.5, 0.5), (0.5, 0.5), (0.0, y), (360.0, 84.0)),
            "Image": {
                "color": [0.16, 0.2, 0.36, 0.92],
                "enabled": True,
                "raycast_target": True,
                "texture": None,
            },
            "Button": {
                "color_multiplier": 1.0,
                "disabled_color": [0.78431374, 0.78431374, 0.78431374, 0.5],
                "highlighted_color": [1.5, 1.5, 1.8, 1.0],
                "interactable": True,
                "normal_color": [1.0, 1.0, 1.0, 1.0],
                "on_click": [{"method": method, "target": target}],
                "pressed_color": [0.7, 0.7, 0.7, 1.0],
            },
        },
    )
    s.add(
        "Text",
        b,
        layer=UI_LAYER,
        **{
            "Rect Transform": rect((0, 0), (1, 1), (0.5, 0.5), (0, 0), (0, 0)),
            "Text": text(caption, 44),
        },
    )
    return b


def screen(title, graph, fields, hint, buttons=()):
    """A full-screen page: the menu or the win screen."""
    s = Scene()
    camera(s, 0.0, 3.5)
    light_and_glow(s)
    backdrop(s, 0.0)
    s.add("Ground", pos=(0.0, -3.5, 0.0), scale=(40.0, 4.0, 1.0),
          **{"Sprite Renderer": sprite(SQUARE, (0.36, 0.62, 0.3, 1.0), -5)})
    s.add("Hero", pos=(-5.5, -1.0, 0.0), scale=(2.0, 2.0, 1.0),
          **{"Sprite Renderer": sprite(PLAYER_PNG, (1.0, 1.0, 1.0, 1.0), 5)})
    owner = s.add(graph, **{"Scripts": scripts((graph, fields))})
    c = canvas(s)
    label(s, c, "Title", title, 120, (0.5, 0.74), (0.0, 0.0), (1400.0, 170.0),
          (1.0, 0.97, 0.85, 1.0))
    first = None
    for i, (caption, method) in enumerate(buttons):
        b = button(s, c, f"{caption} Button", caption, 20.0 - i * 110.0, owner, method)
        first = b if first is None else first
    label(s, c, "Hint", hint, 26, (0.5, 0.08), (0.0, 0.0), (1200.0, 40.0),
          (0.08, 0.1, 0.2, 0.9))
    s.add(
        "EventSystem",
        **{
            "Event System": {
                "drag_threshold": 10.0,
                "enabled": True,
                # Enter submits the selected button; its On Click reaches graphs only with
                # the round 66 fix (graphs.TRIGGERS), until then the Menu graph reads Enter.
                "first_selected": first if graphs.TRIGGERS else None,
                "send_navigation_events": True,
            }
        },
    )
    return s


def burst_prefab():
    s = Scene()
    s.add(
        "Burst",
        **{
            "Particle System": particles(
                burst=26, lifetime=(0.35, 0.7), speed=(2.0, 5.0), size=(0.18, 0.42),
                color=(1.0, 0.75, 0.45, 1.0), looping=False, awake=True, radius=0.25,
                gravity=0.6,
            ),
            "Scripts": scripts(("Vanish", {"After": 1.2})),
        },
    )
    return s


STAGE_1 = [
    "..............................................",
    "..............................................",
    "..............................................",
    "..............................................",
    ".......................###....................",
    "..............................................",
    ".P.........U........B.............U.......G...",
    "###############..############..###############",
    "###############..############..###############",
]

STAGE_2 = [
    "....................................................",
    "....................................................",
    "....................................................",
    "..................U.................................",
    ".............########........###....................",
    ".........##.........................................",
    ".P.....B..............B...........##..U....B....G...",
    "############...###########...######..###############",
    "############...###########...######..###############",
]

STAGE_3 = [
    "..........................................................",
    "..........................................................",
    "..........................................................",
    "......................U.........B.........................",
    "...........###.....######....#######......................",
    ".......##........................................##.......",
    ".P..U........B..........B..............U....B....##...G...",
    "##########..########..#######...###..#####################",
    "##########..########..#######...###..#####################",
]


def all_scenes(demo=False):
    """name → Scene. `demo`: the stages play themselves (Autopilot instead of Player Input)."""
    driver = "Autopilot" if demo else "Player Input"
    return {
        SCENES["menu"]: screen(
            "Lumo's Leap",
            "Menu",
            {},
            "Enter: play    Esc: quit",
            [("Play", "Play"), ("Quit", "Quit")],
        ),
        SCENES["stage1"]: stage(1, STAGE_1, driver),
        SCENES["stage2"]: stage(2, STAGE_2, driver),
        SCENES["stage3"]: stage(3, STAGE_3, driver),
        SCENES["win"]: screen("You Won!", "Win", {}, "Press Enter to go back to the menu"),
    }
