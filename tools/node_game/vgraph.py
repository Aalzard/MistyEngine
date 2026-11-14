"""A small builder for Script Graph (.vgraph) files, used by node_game.py.

It writes exactly what the Script Graph window saves: nodes, wires, Blackboard variables and
sticky notes. Nothing here runs in the game; the graphs do.
"""

import json

FLOW_OUT = {"Delay": "Completed", "DoOnce": "Completed", "Gate": "Exit"}


def lit(v):
    """A Python value as a graph literal."""
    if v is None:
        return "None"
    if isinstance(v, bool):
        return {"Bool": v}
    if isinstance(v, int):
        return {"Int": v}
    if isinstance(v, float):
        return {"Float": v}
    if isinstance(v, str):
        return {"String": v}
    if isinstance(v, (tuple, list)) and len(v) == 3:
        return {"Vector3": [float(x) for x in v]}
    raise TypeError(f"no literal for {v!r}")


class Out:
    """An output pin of a node."""

    def __init__(self, node, pin):
        self.node, self.pin = node, pin


class Node:
    def __init__(self, graph, data):
        self.g, self.d = graph, data

    @property
    def id(self):
        return self.d["id"]

    def __getitem__(self, pin):
        return Out(self, pin)

    def set(self, **pins):
        """Wire or type values into input pins (underscores in names read as spaces)."""
        for name, v in pins.items():
            self.g.inp(self, name.replace("_", " "), v)
        return self

    def to(self, other, out=None, inp="In"):
        """An exec wire from this node to `other`; returns `other`."""
        out = out or FLOW_OUT.get(self.d["type"], "Out")
        self.g.wire(self, out, other, inp)
        return other


class Graph:
    def __init__(self):
        self.nodes, self.conns, self.vars, self.notes = [], [], [], []
        self._band = 0.0
        self._col = 0
        self._pure_row = {}

    # ---- nodes -------------------------------------------------------------------------------
    def node(self, kind, params=None, **pins):
        data = {"id": len(self.nodes) + 1, "type": kind, "position": [0.0, 0.0]}
        if params:
            data["params"] = {k: str(v) for k, v in params.items()}
        self.nodes.append(data)
        n = Node(self, data)
        n.set(**pins)
        return n

    def inp(self, node, pin, v):
        if isinstance(v, Out):
            self.wire(v.node, v.pin, node, pin)
        else:
            node.d.setdefault("values", {})[pin] = lit(v)

    def wire(self, a, out, b, inp):
        self.conns.append({"from": a.id, "output": out, "to": b.id, "input": inp})

    def var(self, name, ty, value, obj=False):
        v = {"name": name, "type": ty, "value": lit(value)}
        if obj:
            v["scope"] = "Object"
        self.vars.append(v)

    def note(self, text, at, size=(420.0, 70.0)):
        self.notes.append({"position": list(at), "size": list(size), "text": text})

    # ---- common nodes ------------------------------------------------------------------------
    def get(self, name):
        return self.node("GetVariable", {"name": name})["Value"]

    def setv(self, name, value):
        return self.node("SetVariable", {"name": name}, Value=value)

    def api_get(self, owner, member, target=None):
        n = self.node("Api", {"owner": owner, "member": member, "mode": "get"})
        if target is not None:
            n.set(Target=target)
        return n["Value"]

    def api_set(self, owner, member, value, target=None):
        n = self.node("Api", {"owner": owner, "member": member, "mode": "set"}, Value=value)
        if target is not None:
            n.set(Target=target)
        return n

    def call(self, owner, member, args="", target=None, **pins):
        params = {"owner": owner, "member": member, "mode": "call"}
        if args:
            params["args"] = args
        n = self.node("Api", params, **pins)
        if target is not None:
            n.set(Target=target)
        return n

    def op(self, kind, **pins):
        """A pure node; returns its Result."""
        return self.node(kind, None, **pins)["Result"]

    def branch(self, cond):
        return self.node("Branch", None, Condition=cond)

    def seq(self, count):
        return self.node("Sequence", {"count": count})

    def delay(self, seconds):
        return self.node("Delay", None, Duration=seconds)

    def event(self, name, args=""):
        p = {"name": name}
        if args:
            p["args"] = args
        return self.node("OnCustomEvent", p)

    def send(self, name, target=None, args="", **pins):
        p = {"name": name}
        if args:
            p["args"] = args
        n = self.node("SendCustomEvent", p, **pins)
        if target is not None:
            n.set(Target=target)
        return n

    def broadcast(self, name, args="", **pins):
        p = {"name": name}
        if args:
            p["args"] = args
        return self.node("BroadcastCustomEvent", p, **pins)

    def vec(self, x=0.0, y=0.0, z=0.0):
        return self.node("MakeVector3", None, X=x, Y=y, Z=z)["Vector"]

    def color(self, r, g, b, a=1.0):
        return self.call("Color", "new", "r, g, b, a", r=r, g=g, b=b, a=a)["Result"]

    # ---- layout ------------------------------------------------------------------------------
    def layout(self):
        """Lay the nodes out: one band per event, exec chains left to right, each node's pure
        inputs stacked to its lower left."""
        by_id = {n["id"]: n for n in self.nodes}
        outs, ins = {}, {}
        for c in self.conns:
            outs.setdefault(c["from"], []).append(c)
            ins.setdefault(c["to"], []).append(c)
        exec_nodes = set()
        for n in self.nodes:
            k = n["type"]
            if k.startswith("On") or k in (
                "Branch", "Sequence", "Delay", "DoOnce", "Gate", "FlipFlop", "ForEach",
                "ForLoop", "While", "SetVariable", "SetObjectVariable", "SendCustomEvent",
                "BroadcastCustomEvent",
            ):
                exec_nodes.add(n["id"])
        for c in self.conns:
            if c["input"] in ("In", "Reset", "Enter", "Open", "Close", "Toggle", "Break"):
                exec_nodes.add(c["to"])
                exec_nodes.add(c["from"])
        placed = {}
        COL, ROW = 330.0, 150.0
        cursor = [0.0]

        def pure_inputs(i):
            return [c["from"] for c in ins.get(i, []) if c["from"] not in exec_nodes]

        def pure_height(i, seen):
            h = 0
            for p in pure_inputs(i):
                if p in seen:
                    continue
                seen.add(p)
                h += 1 + pure_height(p, seen)
            return h

        def place_pures(i, x, y, depth=1):
            row = 0
            for p in pure_inputs(i):
                if p in placed:
                    continue
                placed[p] = (x - 250.0 * depth, y + 95.0 * (row + 1))
                row += 1
                row += place_pures(p, x, y + 95.0 * row, depth + 1)
            return row

        def place(i, col, y):
            """Place exec node `i`; returns the height of its subtree in rows."""
            if i in placed:
                return 0.0
            x = col * COL
            placed[i] = (x, y)
            own = 1.0 + 0.65 * pure_height(i, set())
            place_pures(i, x, y)
            used = 0.0
            for c in outs.get(i, []):
                if c["to"] in exec_nodes and c["input"] in (
                    "In", "Reset", "Enter", "Open", "Close", "Toggle", "Break"
                ):
                    used += place(c["to"], col + 1, y + used * ROW)
            return max(own, used)

        for n in self.nodes:
            if n["type"].startswith("On") and n["id"] not in placed:
                h = place(n["id"], 0, cursor[0])
                cursor[0] += (h + 0.6) * ROW
        for n in self.nodes:
            if n["id"] not in placed:
                placed[n["id"]] = (-300.0, cursor[0])
                cursor[0] += 95.0
        for i, (x, y) in placed.items():
            by_id[i]["position"] = [float(x), float(y)]

    def to_json(self):
        self.layout()
        g = {"version": 1, "nodes": self.nodes, "connections": self.conns}
        if self.vars:
            g["variables"] = self.vars
        if self.notes:
            g["notes"] = self.notes
        return json.dumps(g, indent=2) + "\n"
