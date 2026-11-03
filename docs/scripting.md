# Scripting (Lua)

Scripts are `.lua` files in `Assets/`. Create one with **Project ▸ + ▸ Script** (or Add Component ▸ New script),
attach it by dragging it onto an object (Scene view, Hierarchy, or Inspector ▸ Add Component ▸ Scripts), and
double-click it to edit. Changes are picked up automatically; syntax errors appear in the Console.

```lua
local Rotator = {
    speed = 90,                    -- public fields: number, boolean, string, Vector3
    axis = Vector3.new(0, 1, 0),   -- shown in the Inspector as "Speed", "Axis"
}

function Rotator:Update()
    self.transform:Rotate(self.axis * (self.speed * Time.deltaTime))
end

return Rotator                     -- a script must return its class table
```

## Callbacks
`Awake`, `Start`, `Update`, `FixedUpdate` (every 0.02 s physics step), `LateUpdate`,
`OnCollisionEnter(other)`, `OnCollisionExit(other)`, `OnTriggerEnter(other)`, `OnTriggerExit(other)`
(`other` is a GameObject). Fields starting with `_` stay private.

## API
| | |
|---|---|
| `self.gameObject` | `name`, `tag`, `activeSelf`, `transform`, `SetActive(b)`, `CompareTag(t)`, `GetComponent("Rigidbody"/"Transform")` |
| `self.transform` | `position`, `localPosition`, `eulerAngles`, `localEulerAngles`, `localScale`, `forward`, `right`, `up`, `parent`, `Translate(v)`, `Rotate(v)`, `LookAt(target)` |
| Rigidbody | `velocity`, `angularVelocity`, `mass`, `AddForce(v [, ForceMode.X])`, `AddTorque(v [, mode])` |
| `Vector3` | `new(x,y,z)`, `zero one up down left right forward back`, `Distance`, `Dot`, `Cross`, `Lerp`; `+ - * /`, `.magnitude`, `.normalized` |
| `Time` | `deltaTime`, `time`, `frameCount`, `fixedDeltaTime`, `timeScale`, `unscaledDeltaTime` (read-only; `fixedDeltaTime` and `timeScale` come from Edit ▸ Project Settings) |
| `Input` | `GetKey/GetKeyDown/GetKeyUp("w")`, `GetMouseButton(0)`, `GetAxis("Horizontal"/"Vertical"/"Mouse X"/"Mouse Y")` — the Game view must have focus (click it) |
| `Debug` | `Log`, `LogWarning`, `LogError` (`print` = `Debug.Log`) |
| `Instantiate` | `Instantiate("Assets/Prefabs/Bullet.prefab" [, position])` or `Instantiate(gameObject [, position])` — returns the new "Name(Clone)" GameObject (at the root, with its physics and scripts; its `Awake` runs right after the current callback) |
| `SceneManager` | `LoadScene("Level2")` / `LoadScene(1)` — load a scene from File ▸ Build Settings by name, path or build index (after the current frame; in the editor until Stop); `GetActiveScene().name` |
| `Application` | `Quit()` — closes the built game (ignored with a Console message in the editor) |
| Other | `GameObject.Find(name)`, `Destroy(gameObject)`, `Mathf.*` (`Clamp`, `Lerp`, `Sin`, `PI`, …), `ForceMode.Force/Impulse/VelocityChange/Acceleration` |

Key names: letters/digits (`"a"`, `"1"`), `"space"`, `"return"`, `"escape"`, `"tab"`, `"up"`, `"down"`, `"left"`,
`"right"`, `"left shift"`, `"left ctrl"`, `"left alt"`, `"f1"`…

Samples: `Assets/Scripts/Rotator.lua`, `Assets/Scripts/PlayerController.lua`.

### Spawning objects

```lua
local Spawner = { interval = 1 }
function Spawner:Start() self._t = 0 end
function Spawner:Update()
    self._t = self._t + Time.deltaTime
    if self._t > self.interval then
        self._t = 0
        Instantiate("Assets/Prefabs/Ball.prefab", self.transform.position + Vector3.up * 2)
    end
end
return Spawner
```
Create the prefab by dragging an object from the Hierarchy into the Project window.
