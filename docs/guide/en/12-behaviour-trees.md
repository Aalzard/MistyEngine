# Behaviour Trees: AI for enemies and NPCs

A **Behaviour Tree** decides what a character does next: patrol, chase, attack, look around.
You build it from nodes in the **Behaviour Tree** window, and a **Behaviour Tree Agent**
archetype runs it on an object while the game plays.

Behaviour Trees are an optional package. Open **Window ▸ Package Manager…**, choose
**Built-in**, select **Behaviour Trees** and press **Install**. The window, the
**Assets ▸ Create ▸ Behaviour Tree** menu item and the archetype appear in this project only.

**The example:** `Assets/Scenes/BehaviourTreeDemo.scene` and its tree `Assets/AI/Guard.btree`.
A guard patrols four waypoints, chases the player when it sees them, attacks in range, searches
for a moment when the player hides, and goes back to its patrol. Move the player with WASD.

## How a tree runs

The tree is read from the **Root** on the left to the leaves on the right. Several times a
second the tree is *ticked*: each node runs and answers **Success**, **Failure** or
**Running** (not finished yet, ask again next tick). A parent runs its children from top to
bottom; the small numbers on the wires show the order.

| Family | Nodes | What they do |
|---|---|---|
| Composites | Sequence, Selector, Parallel, Random Selector | run several children |
| Decorators | Inverter, Repeat, Cooldown, Time Limit, Conditional, Force Success, Force Failure | change what one child does |
| Actions | Move To, Wait, Play Animation, Set Animator Parameter, Look At, Set Blackboard, Log | do something |
| Conditions | Is In Range, Can See Target, Has Line of Sight | check something |
| Scripting | Run Script Graph, Run Lua, custom Lua leaves | your own logic |

A **Sequence** is "do this, then this, then this": it stops at the first child that fails. A
**Selector** is "try this, else this": it stops at the first child that succeeds. That is
why the guard's brain is a Selector: *fight* if the player is seen, else *search* after a
chase, else *patrol*.

## The blackboard

The **Blackboard** panel lists the tree's keys: its memory. A key has a name and a type
(Bool, Int, Float, Vector, Object or String). Nodes read and write keys: **Move To** walks to
a Vector or Object key, **Can See Target** stores what it saw in a Bool key, and **Set
Blackboard** writes any value.

Every agent gets its own values. Tick **Shared** on a key to give every agent of the same
share group one value, so one guard that spots the player can alert the others.

In the guard's Behaviour Tree Agent, **Blackboard Overrides** set **Player** to the player
object of the scene.

## Reacting at once: aborts

A **Conditional** runs its child only while a key matches, for example
"SeesPlayer Is Set". Its **Abort** setting makes the tree react the moment the key changes:

- **Self** stops its own branch when the condition turns false (the guard gives up the chase);
- **Lower Priority** stops the branch running below it when the condition turns true (the
  guard stops patrolling as soon as it sees the player);
- **Both** does both.

The check only happens when the key changes, so it costs nothing while nothing happens.

## Your own leaves

- **Run Lua** calls a function of the object's Lua script, such as `Attack`. Return `true`
  for success, `false` for failure or `"running"` to keep going.
- **Run Script Graph** sends a custom event to a Script Graph and waits until the graph uses
  the **BehaviourTree: Finish** node.
- A **custom Lua leaf** is a script that starts with a `@behaviour-leaf` line. It shows in the
  **Nodes** palette under Scripting, with its settings:

```lua
-- @behaviour-leaf Next Waypoint
-- @param waypoints Waypoint 1, Waypoint 2
local NextWaypoint = {}

function NextWaypoint:OnTick(dt)
    BehaviourTree.SetValue(self.gameObject, "PatrolIndex", 1)
    return true
end

return NextWaypoint
```

## Watching it work

Press **Play** and select the guard. In the Behaviour Tree window running nodes glow yellow,
nodes that just succeeded flash green and nodes that failed flash red. The Blackboard panel
shows the agent's **Live values**, such as **State** changing from Patrol to Chase and Attack.

> [!PROGRAMMERS]
> Scripts reach the tree through the `BehaviourTree` table: `GetValue`, `SetValue`, `HasKey`,
> `Start`, `Stop`, `Restart`, `IsRunning`, `GetStatus` and `Finish`. The same functions are
> Script Graph nodes. A tick allocates no memory, and the same seed gives the same random
> choices, so runs repeat exactly.
