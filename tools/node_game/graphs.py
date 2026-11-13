"""The Script Graphs of the node game ("Lumo's Leap"). Every behaviour of the game is here, as
nodes: there is no Lua and no engine-side gameplay code."""

from vgraph import Graph

GRAPHS = "Assets/NodeGame/Graphs"
PREFABS = "Assets/NodeGame/Prefabs"
SCENES = {
    "menu": "NodeGameMenu",
    "stage1": "NodeGameStage1",
    "stage2": "NodeGameStage2",
    "stage3": "NodeGameStage3",
    "win": "NodeGameWin",
}
SND_JUMP = "Assets/Audio/Blip.wav"
SND_DASH = "Assets/Audio/Beep.wav"
SND_HIT = "Assets/Audio/Clap.wav"
SND_WIN = "Assets/Audio/Chime.wav"
SND_HURT = "Assets/Audio/Buzz.wav"

# 2D triggers reach Script Graphs (fixed in round 66). False rebuilds the round 65 workarounds.
TRIGGERS = False

RAY_ARGS = "origin, direction, distance, mask"
# The Default layer only: enemies, the player and the goal are on other layers.
GROUND_MASK = 1


def now(g):
    return g.api_get("Time", "time")


def position(g, target=None):
    return g.api_get("#Transform", "position", target)


def xyz(g, v):
    n = g.node("BreakVector3", None, Vector=v)
    return n["X"], n["Y"], n["Z"]


def const_v3(g, x, y, z):
    return g.node("Vector3", None, Value=(x, y, z))["Value"]


def sound(g, clip, volume=0.6):
    return g.call(
        "AudioSource",
        "PlayClipAtPoint",
        "clipPath, position, volume",
        clipPath=clip,
        position=position(g),
        volume=volume,
    )


def tint(g, r, gr, b, a=1.0, target=None):
    return g.api_set("#SpriteRenderer", "color", g.color(r, gr, b, a), target)


def raycast(g, origin, direction, distance):
    return g.call(
        "Physics2D",
        "Raycast",
        RAY_ARGS,
        origin=origin,
        direction=direction,
        distance=distance,
        mask=GROUND_MASK,
    )


def hit(g, ray):
    """True when a Physics2D.Raycast node hit something."""
    return g.op("NotEqual", A=ray["Result"])


def load_scene(g, name):
    return g.call("SceneManager", "LoadScene", "nameOrIndex, mode", nameOrIndex=name)


def any_key_down(g, keys):
    r = None
    for k in keys:
        d = g.call("Input", "GetKeyDown", Key=k)["Result"]
        r = d if r is None else g.op("Or", A=r, B=d)
    return r


def any_key(g, keys):
    r = None
    for k in keys:
        d = g.call("Input", "GetKey", Key=k)["Result"]
        r = d if r is None else g.op("Or", A=r, B=d)
    return r


# ---- Player ----------------------------------------------------------------------------------


def player_input():
    """Keyboard → the player's Move / Jump Held variables and JumpPressed / DashPressed events."""
    g = Graph()
    g.var("Move", "float", 0.0, obj=True)
    g.var("Jump Held", "bool", False, obj=True)
    up = g.node("OnUpdate")
    s = up.to(g.seq(4))
    s.to(g.setv("Move", g.call("Input", "GetAxis", Axis_Name="Horizontal")["Result"]), "Then 0")
    s.to(g.setv("Jump Held", any_key(g, ["space", "w", "up"])), "Then 1")
    b = s.to(g.branch(any_key_down(g, ["space", "w", "up"])), "Then 2")
    b.to(g.send("JumpPressed"), "True")
    b = s.to(g.branch(any_key_down(g, ["e"])), "Then 3")
    b.to(g.send("DashPressed"), "True")
    return g


def player():
    g = Graph()
    for name, v in [
        ("Speed", 6.0),
        ("Jump Speed", 14.0),
        ("Jump Cut", 0.5),
        ("Coyote Time", 0.1),
        ("Jump Buffer", 0.12),
        ("Dash Speed", 17.0),
        ("Dash Time", 0.18),
        ("Dash Cooldown", 0.45),
        ("Fall Limit", -7.0),
    ]:
        g.var(name, "float", v, obj=True)
    g.var("Hearts", "int", 3, obj=True)
    g.var("Move", "float", 0.0, obj=True)
    g.var("Jump Held", "bool", False, obj=True)
    g.var("Dashing", "bool", False, obj=True)
    g.var("Facing", "float", 1.0, obj=True)
    g.var("Dead", "bool", False, obj=True)
    g.var("Jump Pressed At", "float", -1.0)
    g.var("Grounded At", "float", -1.0)
    g.var("Rising", "bool", False)
    g.var("VY", "float", 0.0)
    g.var("Dash Until", "float", 0.0)
    g.var("Next Dash At", "float", 0.0)
    g.var("Safe Until", "float", 0.0)
    g.var("Spawn", "vector3", (0.0, 0.0, 0.0))

    start = g.node("OnStart")
    start.to(g.setv("Spawn", position(g))).to(
        g.broadcast("HeartsChanged", "hearts", hearts=g.get("Hearts"))
    )

    g.event("JumpPressed").to(g.setv("Jump Pressed At", now(g)))

    # Dash: glow white, leave a trail, fly forward.
    can_dash = g.op(
        "And",
        A=g.op("GreaterOrEqual", A=now(g), B=g.get("Next Dash At")),
        B=g.op("Not", Value=g.get("Dead")),
    )
    d = g.event("DashPressed").to(g.branch(can_dash))
    d = d.to(g.setv("Dashing", True), "True")
    d = d.to(g.setv("Dash Until", g.op("AddFloat", A=now(g), B=g.get("Dash Time"))))
    d = d.to(g.setv("Next Dash At", g.op("AddFloat", A=now(g), B=g.get("Dash Cooldown"))))
    d = d.to(tint(g, 4.0, 4.0, 4.0))
    d = d.to(g.call("#ParticleSystem", "Play"))
    d = d.to(sound(g, SND_DASH, 0.5))
    # The HUD shows when the next dash is ready.
    d.to(g.broadcast("DashUsed", "ready", ready=g.get("Next Dash At")))

    fx = g.node("OnFixedUpdate")
    alive = fx.to(g.branch(g.get("Dead")))
    s = alive.to(g.seq(5), "False")

    # 0: on the ground? Three short rays down from the feet.
    pos = position(g)
    down = const_v3(g, 0.0, -1.0, 0.0)
    rays = [
        raycast(g, g.op("AddVector3", A=pos, B=(dx, -0.45, 0.0)), down, 0.12)
        for dx in (-0.3, 0.0, 0.3)
    ]
    s.to(rays[0], "Then 0").to(rays[1]).to(rays[2])
    grounded = g.op("Or", A=g.op("Or", A=hit(g, rays[0]), B=hit(g, rays[1])), B=hit(g, rays[2]))
    rays[2].to(g.branch(grounded)).to(g.setv("Grounded At", now(g)), "True")

    # 1: face the way we move.
    moving = g.op("Greater", A=g.op("AbsFloat", Value=g.get("Move")), B=0.01)
    f = s.to(g.branch(moving), "Then 1")
    f = f.to(g.setv("Facing", g.op("Sign", Value=g.get("Move"))), "True")
    f.to(g.api_set("#SpriteRenderer", "flipX", g.op("Less", A=g.get("Move"), B=0.0)))

    # 2: the dash ends.
    over = g.op(
        "And", A=g.get("Dashing"), B=g.op("GreaterOrEqual", A=now(g), B=g.get("Dash Until"))
    )
    e = s.to(g.branch(over), "Then 2")
    e = e.to(g.setv("Dashing", False), "True")
    # A moment of safety after a dash, so the enemy it knocked back cannot hurt at once.
    e = e.to(g.setv("Safe Until", g.op("AddFloat", A=now(g), B=0.35)))
    e = e.to(tint(g, 1.0, 1.0, 1.0))
    e.to(g.call("#ParticleSystem", "Stop"))

    # 3: velocity. Dashing flies straight; otherwise run, jump (buffered, with coyote time)
    # and cut the jump short when the key is let go.
    _, vy, _ = xyz(g, g.api_get("#Rigidbody2D", "velocity"))
    dash = s.to(g.branch(g.get("Dashing")), "Then 3")
    dash.to(
        g.api_set(
            "#Rigidbody2D",
            "velocity",
            g.vec(g.op("MultiplyFloat", A=g.get("Facing"), B=g.get("Dash Speed")), 0.0, 0.0),
        ),
        "True",
    )

    def recent(var, window):
        return g.op(
            "And",
            A=g.op("GreaterOrEqual", A=g.get(var), B=0.0),
            B=g.op(
                "LessOrEqual", A=g.op("SubtractFloat", A=now(g), B=g.get(var)), B=g.get(window)
            ),
        )

    jump = dash.to(
        g.branch(
            g.op(
                "And",
                A=recent("Jump Pressed At", "Jump Buffer"),
                B=recent("Grounded At", "Coyote Time"),
            )
        ),
        "False",
    )
    apply = g.api_set(
        "#Rigidbody2D",
        "velocity",
        g.vec(g.op("MultiplyFloat", A=g.get("Move"), B=g.get("Speed")), g.get("VY"), 0.0),
    )
    j = jump.to(g.setv("VY", g.get("Jump Speed")), "True")
    j = j.to(g.setv("Jump Pressed At", -1.0))
    j = j.to(g.setv("Grounded At", -1.0))
    j = j.to(g.setv("Rising", True))
    j.to(sound(g, SND_JUMP, 0.4)).to(apply)
    let_go = g.op(
        "And",
        A=g.op("And", A=g.get("Rising"), B=g.op("Greater", A=vy, B=0.0)),
        B=g.op("Not", Value=g.get("Jump Held")),
    )
    cut = jump.to(g.branch(let_go), "False")
    c = cut.to(g.setv("VY", g.op("MultiplyFloat", A=vy, B=g.get("Jump Cut"))), "True")
    c.to(g.setv("Rising", False)).to(apply)
    cut.to(g.setv("VY", vy), "False").to(apply)
    apply.to(g.branch(g.op("LessOrEqual", A=g.get("VY"), B=0.0))).to(
        g.setv("Rising", False), "True"
    )

    # 4: fell into a pit.
    _, py, _ = xyz(g, position(g))
    s.to(g.branch(g.op("Less", A=py, B=g.get("Fall Limit"))), "Then 4").to(
        g.send("Fell"), "True"
    )

    def lose_heart(after):
        """Hearts - 1, tell the HUD; out of hearts → PlayerDied. Returns the Branch (True =
        dead)."""
        n = after.to(g.setv("Hearts", g.op("SubtractInt", A=g.get("Hearts"), B=1)))
        n = n.to(g.broadcast("HeartsChanged", "hearts", hearts=g.get("Hearts")))
        n = n.to(sound(g, SND_HURT, 0.35))
        b = n.to(g.branch(g.op("LessOrEqual", A=g.get("Hearts"), B=0)))
        dead = b.to(g.setv("Dead", True), "True")
        dead = dead.to(tint(g, 0.35, 0.35, 0.4))
        dead = dead.to(g.api_set("#Rigidbody2D", "velocity", g.vec(0.0, 6.0, 0.0)))
        dead.to(g.broadcast("PlayerDied"))
        return b

    # Touched an enemy without dashing.
    can_hurt = g.op(
        "And",
        A=g.op("GreaterOrEqual", A=now(g), B=g.get("Safe Until")),
        B=g.op(
            "And", A=g.op("Not", Value=g.get("Dashing")), B=g.op("Not", Value=g.get("Dead"))
        ),
    )
    h = g.event("Hurt").to(g.branch(can_hurt))
    h = h.to(g.setv("Safe Until", g.op("AddFloat", A=now(g), B=1.0)), "True")
    b = lose_heart(h)
    k = b.to(tint(g, 1.0, 0.35, 0.35), "False")
    k = k.to(
        g.api_set(
            "#Rigidbody2D",
            "velocity",
            g.vec(g.op("MultiplyFloat", A=g.get("Facing"), B=-4.0), 9.0, 0.0),
        )
    )
    k.to(g.delay(0.3)).to(g.branch(g.get("Dashing"))).to(tint(g, 1.0, 1.0, 1.0), "False")

    # Fell: lose a heart and go back to where the stage started.
    fell = g.event("Fell").to(g.branch(g.get("Dead")))
    b = lose_heart(fell.to(g.setv("Dashing", False), "False"))
    r = b.to(g.api_set("#Transform", "position", g.get("Spawn")), "False")
    r = r.to(g.api_set("#Rigidbody2D", "velocity", g.vec(0.0, 0.0, 0.0)))
    r.to(tint(g, 1.0, 1.0, 1.0)).to(g.call("#ParticleSystem", "Stop"))
    return g


def autopilot():
    """Plays the stage by itself (the demo stages use it in place of Player Input): runs right,
    jumps when the ground ends or a wall is ahead, dashes when an enemy is close."""
    g = Graph()
    g.var("Move", "float", 0.0, obj=True)
    g.var("Jump Held", "bool", False, obj=True)
    g.var("Start After", "float", 0.5, obj=True)
    g.var("Dash Range", "float", 1.2, obj=True)
    up = g.node("OnUpdate")
    go = up.to(g.branch(g.op("Greater", A=now(g), B=g.get("Start After"))))
    s = go.to(g.seq(3), "True")
    s.to(g.setv("Move", 1.0), "Then 0").to(g.setv("Jump Held", True))
    pos = position(g)
    down = const_v3(g, 0.0, -1.0, 0.0)
    under = raycast(g, pos, down, 0.7)
    ahead = raycast(g, g.op("AddVector3", A=pos, B=(0.9, 0.0, 0.0)), down, 1.5)
    wall = raycast(g, pos, const_v3(g, 1.0, 0.0, 0.0), 1.0)
    s.to(under, "Then 1").to(ahead).to(wall)
    # Standing, and the ground ends or a wall is in the way: jump.
    must_jump = g.op(
        "And",
        A=hit(g, under),
        B=g.op("Or", A=g.op("Not", Value=hit(g, ahead)), B=hit(g, wall)),
    )
    wall.to(g.branch(must_jump)).to(g.send("JumpPressed"), "True")
    # Anything close ahead that has a `Dead` variable (the Enemy Touch graph) is an enemy.
    around = g.call(
        "Physics2D",
        "OverlapCircleAll",
        "c, r, m",
        c=g.op("AddVector3", A=position(g), B=(1.3, 0.0, 0.0)),
        r=g.get("Dash Range"),
    )
    each = s.to(around, "Then 2").to(g.node("ForEach", None, List=around["Result"]))
    reach = g.node("GetObjectVariable", None, Target=each["Item"], Name="Dead")["Value"]
    ex, _, _ = xyz(g, position(g, each["Item"]))
    px, _, _ = xyz(g, position(g))
    is_enemy = g.op("And", A=g.op("NotEqual", A=reach), B=g.op("Greater", A=ex, B=px))
    each.to(g.branch(is_enemy), "Loop Body").to(g.send("DashPressed"), "True")
    return g


# ---- Enemies ---------------------------------------------------------------------------------


def patrol():
    """Walk; turn round at a ledge or a wall. `Knock` (set by a hit) slides the enemy back."""
    g = Graph()
    g.var("Speed", "float", 1.5, obj=True)
    g.var("Direction", "float", -1.0, obj=True)
    g.var("Knock", "float", 0.0, obj=True)
    g.var("Dead", "bool", False, obj=True)
    fx = g.node("OnFixedUpdate")
    knocked = g.op("Greater", A=g.op("AbsFloat", Value=g.get("Knock")), B=0.05)
    walk = g.op("MultiplyFloat", A=g.get("Direction"), B=g.get("Speed"))
    sel = g.node("Select", None, Condition=knocked)
    g.inp(sel, "True", g.get("Knock"))
    g.inp(sel, "False", walk)
    vx = sel["Result"]
    side = g.op("Sign", Value=vx)
    pos = position(g)
    ledge = raycast(
        g,
        g.op("AddVector3", A=pos, B=g.vec(g.op("MultiplyFloat", A=side, B=0.55), 0.0, 0.0)),
        const_v3(g, 0.0, -1.0, 0.0),
        1.0,
    )
    wall = raycast(g, pos, g.vec(side, 0.0, 0.0), 0.65)
    live = fx.to(g.branch(g.get("Dead")))
    live.to(ledge, "False").to(wall)
    blocked = g.op("Or", A=g.op("Not", Value=hit(g, ledge)), B=hit(g, wall))
    b = wall.to(g.branch(blocked))
    k = b.to(g.branch(knocked), "True")
    k.to(g.setv("Knock", 0.0), "True")
    turn = k.to(g.setv("Direction", g.op("NegateFloat", Value=g.get("Direction"))), "False")
    turn.to(g.api_set("#SpriteRenderer", "flipX", g.op("Greater", A=g.get("Direction"), B=0.0)))
    move = b.to(
        g.call(
            "#Transform",
            "Translate",
            Translation=g.vec(g.op("MultiplyFloat", A=vx, B=fx["Delta Time"]), 0.0, 0.0),
        ),
        "False",
    )
    move.to(
        g.setv(
            "Knock",
            g.op(
                "MoveTowards",
                Current=g.get("Knock"),
                Target=0.0,
                Max_Delta=g.op("MultiplyFloat", A=fx["Delta Time"], B=14.0),
            ),
        )
    )
    return g


def enemy_touch():
    """The player touches this enemy: a dash hits the enemy, anything else hurts the player."""
    g = Graph()
    g.var("Dead", "bool", False, obj=True)
    g.var("Player", "gameobject", None)
    g.var("Touching", "bool", False)
    g.var("Next Hit At", "float", 0.0)
    if TRIGGERS:
        # The enemy's collider is a trigger: remember the player while it is inside.
        enter = g.node("OnTriggerEnter")
        is_player = g.call("#GameObject", "CompareTag", target=enter["Other"], Tag="Player")["Result"]
        e = enter.to(g.branch(is_player)).to(g.setv("Player", enter["Other"]), "True")
        e.to(g.setv("Touching", True))
        leave = g.node("OnTriggerExit")
        was_player = g.call("#GameObject", "CompareTag", target=leave["Other"], Tag="Player")["Result"]
        leave.to(g.branch(was_player)).to(g.setv("Touching", False), "True")
        touching = g.get("Touching")
    else:
        # Round 65 workaround (2D triggers did not reach graphs): a distance check.
        g.var("Reach", "float", 0.85, obj=True)
        g.node("OnStart").to(
            g.setv("Player", g.call("GameObject", "Find", Name="Player")["Result"])
        )
        touching = g.op(
            "Less",
            A=g.op("Distance", A=position(g), B=position(g, g.get("Player"))),
            B=g.get("Reach"),
        )
    up = g.node("OnUpdate")
    ok = g.op(
        "And",
        A=g.op("IsValid", Object=g.get("Player")),
        B=g.op("Not", Value=g.get("Dead")),
    )
    t = up.to(g.branch(ok)).to(g.branch(touching), "True")
    dashing = g.node("GetObjectVariable", None, Target=g.get("Player"), Name="Dashing")["Value"]
    d = t.to(g.branch(dashing), "True")
    ready = d.to(g.branch(g.op("GreaterOrEqual", A=now(g), B=g.get("Next Hit At"))), "True")
    r = ready.to(g.setv("Next Hit At", g.op("AddFloat", A=now(g), B=0.4)), "True")
    facing = g.node("GetObjectVariable", None, Target=g.get("Player"), Name="Facing")["Value"]
    r.to(g.send("DashHit", None, "direction", direction=facing))
    d.to(g.send("Hurt", g.get("Player")), "False")
    return g


def brute():
    """Two dash hits: each one flashes it white and knocks it back; the second destroys it."""
    g = Graph()
    g.var("Health", "int", 2, obj=True)
    g.var("Knock", "float", 0.0, obj=True)
    g.var("Dead", "bool", False, obj=True)
    ev = g.event("DashHit", "direction")
    n = ev.to(g.setv("Health", g.op("SubtractInt", A=g.get("Health"), B=1)))
    n = n.to(g.setv("Knock", g.op("MultiplyFloat", A=ev["direction"], B=11.0)))
    n = n.to(sound(g, SND_HIT, 0.6))
    n = n.to(tint(g, 5.0, 5.0, 5.0))
    b = n.to(g.branch(g.op("LessOrEqual", A=g.get("Health"), B=0)))
    die = b.to(g.setv("Dead", True), "True")
    die = die.to(
        g.call(
            "",
            "Instantiate",
            "prefabPathOrObject, position",
            prefabPathOrObject=f"{PREFABS}/Burst.prefab",
            position=position(g),
        )
    )
    die.to(g.call("", "Destroy", Object=g.node("Self")["Self"]))
    b.to(g.delay(0.14), "False").to(tint(g, 0.78, 0.22, 0.2))
    return g


def puff():
    """One dash hit pushes it away; it swells, fades out and is destroyed."""
    g = Graph()
    g.var("Dead", "bool", False, obj=True)
    g.var("Push", "float", 0.0)
    g.var("Fade", "float", 1.0)
    ev = g.event("DashHit", "direction")
    n = ev.to(g.branch(g.get("Dead")))
    n = n.to(g.setv("Dead", True), "False")
    n = n.to(g.setv("Push", g.op("MultiplyFloat", A=ev["direction"], B=7.0)))
    n.to(sound(g, SND_HIT, 0.5))
    up = g.node("OnUpdate")
    dt = up["Delta Time"]
    d = up.to(g.branch(g.get("Dead")))
    d = d.to(
        g.call(
            "#Transform",
            "Translate",
            Translation=g.vec(
                g.op("MultiplyFloat", A=g.get("Push"), B=dt), g.op("MultiplyFloat", A=dt, B=2.0), 0.0
            ),
        ),
        "True",
    )
    d = d.to(
        g.setv(
            "Fade",
            g.op(
                "MoveTowards",
                Current=g.get("Fade"),
                Target=0.0,
                Max_Delta=g.op("MultiplyFloat", A=dt, B=1.7),
            ),
        )
    )
    d = d.to(tint(g, 0.75, 0.6, 1.0, g.get("Fade")))
    size = g.op("Lerp", A=1.5, B=0.8, T=g.get("Fade"))
    d = d.to(g.api_set("#Transform", "localScale", g.vec(size, size, 1.0)))
    d.to(g.branch(g.op("LessOrEqual", A=g.get("Fade"), B=0.0))).to(
        g.call("", "Destroy", Object=g.node("Self")["Self"]), "True"
    )
    return g


# ---- Stage -----------------------------------------------------------------------------------


def goal():
    """The flag at the end of a stage: the player reaching it broadcasts StageCleared (once)."""
    g = Graph()
    if TRIGGERS:
        enter = g.node("OnTriggerEnter")
        is_player = g.call("#GameObject", "CompareTag", target=enter["Other"], Tag="Player")["Result"]
        here = enter.to(g.branch(is_player))
    else:
        # Round 65 workaround (2D triggers did not reach graphs): compare positions.
        g.var("Reach", "float", 0.8, obj=True)
        g.var("Height", "float", 3.0, obj=True)
        g.var("Player", "gameobject", None)
        g.node("OnStart").to(
            g.setv("Player", g.call("GameObject", "Find", Name="Player")["Result"])
        )
        up = g.node("OnUpdate")
        px, py, _ = xyz(g, position(g, g.get("Player")))
        x, y, _ = xyz(g, position(g))
        beside = g.op(
            "Less", A=g.op("AbsFloat", Value=g.op("SubtractFloat", A=px, B=x)), B=g.get("Reach")
        )
        level = g.op(
            "Less",
            A=g.op("AbsFloat", Value=g.op("SubtractFloat", A=py, B=y)),
            B=g.get("Height"),
        )
        here = up.to(g.branch(g.op("IsValid", Object=g.get("Player")))).to(
            g.branch(g.op("And", A=beside, B=level)), "True"
        )
    here.to(g.node("DoOnce"), "True").to(g.broadcast("StageCleared"))
    return g


def stage():
    """One per stage: the HUD, what happens when the stage is cleared or the player dies."""
    g = Graph()
    g.var("This Scene", "string", SCENES["stage1"], obj=True)
    g.var("Next Scene", "string", SCENES["stage2"], obj=True)
    g.var("Done", "bool", False)
    g.var("Dash Ready At", "float", 0.0)

    def find(name):
        return g.call("GameObject", "Find", Name=name)["Result"]

    h = g.event("HeartsChanged", "hearts")
    h.to(
        g.api_set(
            "#Text", "text", g.op("Append", A="Hearts: ", B=h["hearts"]), find("Hearts Text")
        )
    )
    c = g.event("StageCleared").to(g.branch(g.get("Done")))
    c = c.to(g.setv("Done", True), "False")
    c = c.to(sound(g, SND_WIN, 0.6))
    c = c.to(g.api_set("#Text", "text", "Stage clear!", find("Banner Text")))
    c.to(g.delay(1.2)).to(load_scene(g, g.get("Next Scene")))
    d = g.event("PlayerDied").to(g.branch(g.get("Done")))
    d = d.to(g.setv("Done", True), "False")
    d = d.to(g.api_set("#Text", "text", "Try again", find("Banner Text")))
    d.to(g.delay(1.0)).to(load_scene(g, g.get("This Scene")))
    du = g.event("DashUsed", "ready")
    du.to(g.setv("Dash Ready At", du["ready"]))
    up = g.node("OnUpdate")
    s = up.to(g.seq(2))
    s.to(g.branch(any_key_down(g, ["escape"])), "Then 0").to(
        load_scene(g, SCENES["menu"]), "True"
    )
    # Dash cooldown: "Dash: ready" when E can dash again, "Dash: ..." while it recharges.
    ready = s.to(
        g.branch(g.op("GreaterOrEqual", A=now(g), B=g.get("Dash Ready At"))), "Then 1"
    )
    dash_text = find("Dash Text")
    ready.to(g.api_set("#Text", "text", "Dash: ready", dash_text), "True")
    ready.to(g.api_set("#Text", "text", "Dash: ...", dash_text), "False")
    return g


def camera_follow():
    """Follow the player sideways (smoothly, inside Min X..Max X) and a little upwards."""
    g = Graph()
    g.var("Min X", "float", 0.0, obj=True)
    g.var("Max X", "float", 30.0, obj=True)
    g.var("Height", "float", 3.5, obj=True)
    g.var("Target", "gameobject", None)
    g.node("OnStart").to(g.setv("Target", g.call("GameObject", "Find", Name="Player")["Result"]))
    late = g.node("OnLateUpdate")
    cx, cy, cz = xyz(g, position(g))
    tx, ty, _ = xyz(g, position(g, g.get("Target")))
    t = g.op("Clamp01", Value=g.op("MultiplyFloat", A=g.api_get("Time", "deltaTime"), B=6.0))
    x = g.op(
        "Lerp",
        A=cx,
        B=g.op("ClampFloat", Value=tx, Min=g.get("Min X"), Max=g.get("Max X")),
        T=t,
    )
    y = g.op(
        "Lerp",
        A=cy,
        B=g.op("MaxFloat", A=g.get("Height"), B=g.op("SubtractFloat", A=ty, B=1.0)),
        T=t,
    )
    late.to(g.branch(g.op("IsValid", Object=g.get("Target")))).to(
        g.api_set("#Transform", "position", g.vec(x, y, cz)), "True"
    )
    return g


# ---- Menu and win screen ---------------------------------------------------------------------


def menu():
    """Play and Quit: the buttons' On Click send the Play / Quit events; Enter and Escape do
    the same."""
    g = Graph()
    g.var("Started", "bool", False)
    up = g.node("OnUpdate")
    s = up.to(g.seq(3))
    s.to(g.branch(any_key_down(g, ["return", "space"])), "Then 0").to(g.send("Play"), "True")
    s.to(g.branch(any_key_down(g, ["escape"])), "Then 1").to(g.send("Quit"), "True")
    # The title breathes.
    wave = g.op("Sin", Radians=g.op("MultiplyFloat", A=now(g), B=2.5))
    size = g.op("AddFloat", A=1.0, B=g.op("MultiplyFloat", A=wave, B=0.04))
    s.to(
        g.api_set(
            "#Transform",
            "localScale",
            g.vec(size, size, 1.0),
            g.call("GameObject", "Find", Name="Title")["Result"],
        ),
        "Then 2",
    )
    p = g.event("Play").to(g.branch(g.get("Started")))
    p = p.to(g.setv("Started", True), "False")
    p = p.to(sound(g, SND_DASH, 0.5))
    p.to(g.delay(0.15)).to(load_scene(g, SCENES["stage1"]))
    g.event("Quit").to(g.call("Application", "Quit"))
    return g


def win():
    """You Won! Back to the menu after a few seconds or on a key."""
    g = Graph()
    g.var("Leaving", "bool", False)
    g.var("Wait", "float", 6.0, obj=True)
    st = g.node("OnStart").to(sound(g, SND_WIN, 0.7))
    st.to(g.delay(g.get("Wait"))).to(g.send("Back"))
    up = g.node("OnUpdate")
    s = up.to(g.seq(2))
    s.to(g.branch(any_key_down(g, ["return", "space", "escape"])), "Then 0").to(
        g.send("Back"), "True"
    )
    wave = g.op("Sin", Radians=g.op("MultiplyFloat", A=now(g), B=3.0))
    size = g.op("AddFloat", A=1.0, B=g.op("MultiplyFloat", A=wave, B=0.06))
    s.to(
        g.api_set(
            "#Transform",
            "localScale",
            g.vec(size, size, 1.0),
            g.call("GameObject", "Find", Name="Title")["Result"],
        ),
        "Then 1",
    )
    b = g.event("Back").to(g.branch(g.get("Leaving")))
    b.to(g.setv("Leaving", True), "False").to(load_scene(g, SCENES["menu"]))
    return g


def vanish():
    """On the Burst prefab: remove the object once its particles are gone."""
    g = Graph()
    g.var("After", "float", 1.2, obj=True)
    g.node("OnStart").to(g.delay(g.get("After"))).to(
        g.call("", "Destroy", Object=g.node("Self")["Self"])
    )
    return g


ALL = {
    "Vanish": vanish,
    "Player": player,
    "Player Input": player_input,
    "Autopilot": autopilot,
    "Patrol": patrol,
    "Enemy Touch": enemy_touch,
    "Brute": brute,
    "Puff": puff,
    "Goal": goal,
    "Stage": stage,
    "Camera Follow": camera_follow,
    "Menu": menu,
    "Win": win,
}
