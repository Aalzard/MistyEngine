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
(`other` is a GameObject), `OnControllerColliderHit(hit)` (see Character Controller below). Fields starting
with `_` stay private.
Animation events call a function by name (no arguments) on the scripts of the object with the Animation.

## API
| | |
|---|---|
| `self.gameObject` | `name`, `tag`, `layer` (index, settable), `activeSelf`, `transform`, `SetActive(b)`, `CompareTag(t)`, `GetComponent("Rigidbody"/"Transform"/"AudioSource"/"Animation"/"CharacterController"/…)` (nil if the object has none) |
| `self.transform` | `position`, `localPosition`, `eulerAngles`, `localEulerAngles`, `localScale`, `forward`, `right`, `up`, `parent`, `Translate(v)`, `Rotate(v)`, `LookAt(target)` |
| Rigidbody | `velocity`, `angularVelocity`, `mass`, `position` (physics pose, read-only), `isKinematic`, `useGravity`, `AddForce(v [, ForceMode.X])`, `AddTorque(v [, mode])`, `MovePosition(v)`, `MoveRotation(eulerDegrees)` |
| `Physics` | `Raycast`, `RaycastAll`, `SphereCast`, `OverlapSphere` (see Physics queries), `DefaultRaycastLayers`, `AllLayers`, `IgnoreRaycastLayer` |
| `LayerMask` | `GetMask("Default", "Player", …)`, `NameToLayer(name)` (-1 if unknown), `LayerToName(index)` |
| CharacterController | `Move(motion)`, `SimpleMove(speed)` (returns `isGrounded`), `isGrounded`, `velocity`, `height`, `radius`, `center`, `slopeLimit`, `stepOffset`, `skinWidth` (read-only) |
| AudioSource | `Play()`, `Stop()`, `Pause()`, `UnPause()`, `PlayOneShot(clipPath [, volumeScale])`, `isPlaying` (read-only), `volume`, `pitch`, `loop`, `mute`, `spatialBlend`, `enabled`, `clip` (project path like `"Assets/Audio/Beep.wav"`, or nil) |
| Animation | `Play([name])` (the default clip, or the clip of the Animation's list with that file name; restarts it; returns false and warns if missing), `Stop()` (rewinds, keeps the pose), `IsPlaying(name)`, `isPlaying` (read-only), `time` (seconds since Play, keeps growing while looping; settable), `speed` (1 = normal, negative plays backwards), `clip` (name of the current clip, read-only), `enabled` |
| Text (UI) | `GetComponent("Text")`: `text` (numbers are shown as text), `fontSize`, `color`, `enabled`, `gameObject` |
| Image (UI) | `GetComponent("Image")`: `color`, `texture` (project path or nil), `raycastTarget`, `enabled`, `gameObject` |
| Button (UI) | `GetComponent("Button")`: `interactable`, `gameObject`; clicks call the On Click methods set in the Inspector |
| `Color` | `new(r,g,b [,a])` (0..1), `white black red green blue yellow cyan magenta gray clear`, `Lerp(a,b,t)`; `.r .g .b .a`, `* number/Color`, `==` |
| `Vector3` | `new(x,y,z)`, `zero one up down left right forward back`, `Distance`, `Dot`, `Cross`, `Lerp`; `+ - * /`, `.magnitude`, `.normalized` |
| `Time` | `deltaTime`, `time`, `frameCount`, `fixedDeltaTime`, `timeScale`, `unscaledDeltaTime` (read-only; `fixedDeltaTime` and `timeScale` come from Edit ▸ Project Settings) |
| `Input` | `GetKey/GetKeyDown/GetKeyUp("w")`, `GetMouseButton(0)`, `GetAxis("Horizontal"/"Vertical"/"Mouse X"/"Mouse Y")` — the Game view must have focus (click it) |
| `Debug` | `Log`, `LogWarning`, `LogError` (`print` = `Debug.Log`) |
| `Instantiate` | `Instantiate("Assets/Prefabs/Bullet.prefab" [, position])` or `Instantiate(gameObject [, position])` — returns the new "Name(Clone)" GameObject (at the root, with its physics and scripts; its `Awake` runs right after the current callback) |
| `SceneManager` | `LoadScene("Level2")` / `LoadScene(1)` — load a scene from File ▸ Build Settings by name, path or build index (after the current frame; in the editor until Stop); `GetActiveScene().name` |
| `Application` | `Quit()` — closes the built game (ignored with a Console message in the editor) |
| Other | `GameObject.Find(name)`, `Destroy(gameObject)`, `Mathf.*` (`Clamp`, `Lerp`, `Sin`, `PI`, `Infinity`, …), `ForceMode.Force/Impulse/VelocityChange/Acceleration` |

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

### Playing sounds

```lua
local Clicker = {}
function Clicker:Start() self._audio = self.gameObject:GetComponent("AudioSource") end
function Clicker:Update()
    if Input.GetKeyDown("space") then self._audio:PlayOneShot("Assets/Audio/Beep.wav") end
    if Input.GetKeyDown("p") then
        if self._audio.isPlaying then self._audio:Pause() else self._audio:Play() end
    end
end
return Clicker
```
`Play()` restarts the source's clip; `PlayOneShot` plays an extra copy through the same source (its volume,
pitch and 3D position) without interrupting it. Changes to `volume`, `pitch`, `loop` take effect the same frame;
a new `clip` is used by the next `Play()`.

### Controlling animations

```lua
local Door = { }
function Door:Start() self._anim = self.gameObject:GetComponent("Animation") end
function Door:Update()
    if Input.GetKeyDown("e") then self._anim:Play("Open") end
    if Input.GetKeyDown("q") then self._anim.speed = -1 end   -- play the current clip backwards
end
function Door:Opened() Debug.Log("door open") end  -- an animation event in Open.anim
return Door
```
Animations are sampled after every `Update` and before `LateUpdate`, so `LateUpdate` sees the animated pose and
can override it. Sample: `Assets/Scripts/AnimationControl.lua` in `Scenes/AnimationDemo.scene` (Space pauses /
resumes, Up/Down change the speed).

### UI buttons

Create a button with **GameObject ▸ UI ▸ Button** (a Canvas is added if the scene has none). In its Button
component, press **+** under **On Click ()**, pick the object that has your script and type the method name:

```lua
local ScoreKeeper = { scoreText = "Score Text" }
function ScoreKeeper:Start()
    self._score = 0
    self._label = GameObject.Find(self.scoreText):GetComponent("Text")
end
function ScoreKeeper:AddPoint()            -- On Click: Score Keeper ▸ AddPoint
    self._score = self._score + 1
    self._label.text = "Score: " .. self._score
end
return ScoreKeeper
```
A click is a press and release over the same button while it is `interactable`; every script on the target object
that defines the method gets the call (before this frame's `Update`). Sample: `Assets/Scenes/UIDemo.scene` with
`Assets/Scripts/ScoreKeeper.lua`.

### Particle systems

`GetComponent("ParticleSystem")` returns the object's Particle System (or `nil`):

| Member | Meaning |
| --- | --- |
| `Play()` | Start a new cycle, or resume when paused (live particles are kept). |
| `Stop()` | Stop emitting; existing particles finish their lives. |
| `Pause()` | Freeze the simulation until `Play()`. |
| `Clear()` | Remove every live particle. |
| `Emit(n)` | Emit `n` particles now, whatever the play state (up to Max Particles). |
| `isPlaying`, `isPaused`, `isStopped` | Play state (`isStopped` = stopped and no particles left). |
| `particleCount` | Live particles. |
| `time` | Seconds into the current cycle. |

```lua
local Sparks = { amount = 40 }
function Sparks:Start() self.ps = self.gameObject:GetComponent("ParticleSystem") end
function Sparks:Update()
    if Input.GetKeyDown("space") then self.ps:Emit(self.amount) end
    if Input.GetKeyDown("p") then
        if self.ps.isPlaying then self.ps:Pause() else self.ps:Play() end
    end
end
return Sparks
```
Systems with Play On Awake are already playing in `Awake`/`Start`. Module settings are not scriptable yet.

### Physics queries

```lua
local hit = Physics.Raycast(origin, direction [, maxDistance [, layerMask]])   -- nil when nothing is hit
if hit ~= nil then
    Debug.Log(hit.gameObject.name, hit.distance, hit.point, hit.normal)
end
local all = Physics.RaycastAll(origin, direction, 50)          -- list of hits, nearest first
local hit2 = Physics.SphereCast(origin, 0.5, direction, 10)    -- like Raycast with a sphere
local near = Physics.OverlapSphere(center, 3)                  -- list of GameObjects (one per collider)
```
A hit has `point`, `normal`, `distance`, `gameObject`, `collider` (the collider's GameObject, so
`hit.collider.gameObject` works), `transform` and `rigidbody` (nil for static colliders). `direction` doesn't need
to be normalised; `maxDistance` defaults to `Mathf.Infinity`. Masks are layer bit masks: Raycast/RaycastAll/
SphereCast default to `Physics.DefaultRaycastLayers` (every layer but Ignore Raycast), OverlapSphere to every layer.
Combine them with Lua's bit operators, e.g. `Physics.DefaultRaycastLayers & ~LayerMask.GetMask("Player")`.
Rays ignore colliders they start inside. Triggers are hit unless Project Settings ▸ Physics ▸ Queries Hit
Triggers is off; the Layer Collision Matrix there decides which layers collide (queries use only the mask).
Queries see the positions of the last physics step (and work in `Awake`/`Start`).

### Character Controller

Add **Component ▸ Physics ▸ Character Controller** (it is its own capsule collider; remove other colliders or
put them on another layer). Move it from a script; it slides along walls, climbs steps up to Step Offset and
slopes up to Slope Limit, and isn't pushed by physics.

```lua
local Walker = { speed = 4, gravity = -15 }
function Walker:Start() self.cc = self.gameObject:GetComponent("CharacterController"); self._vy = 0 end
function Walker:Update()
    local move = self.transform.forward * (Input.GetAxis("Vertical") * self.speed)
    if self.cc.isGrounded then self._vy = -1 end
    self._vy = self._vy + self.gravity * Time.deltaTime
    self.cc:Move((move + Vector3.new(0, self._vy, 0)) * Time.deltaTime)   -- motion for this frame
end
function Walker:OnControllerColliderHit(hit)                              -- push crates
    if hit.rigidbody ~= nil then hit.rigidbody.velocity = hit.moveDirection * 2 end
end
return Walker
```
`Move(motion)` moves by a world-space displacement (no gravity added) and updates the Transform at once;
`SimpleMove(speed)` takes a velocity in units/second, ignores its Y and applies gravity itself. `isGrounded` and
`velocity` describe the last move. `OnControllerColliderHit(hit)` is called once per collider touched by a move,
right after the callback that called `Move`; `hit` has `gameObject`, `collider`, `transform`, `rigidbody`, `point`,
`normal`, `moveDirection`, `moveLength`, `controller`. Sample: `Scenes/PhysicsDemo.scene` with
`Assets/Scripts/CharacterWalker.lua` and `Assets/Scripts/LaserPointer.lua`.

### Joints

Fixed, Hinge and Spring Joints are set up in the Inspector (Connected Body = another object with a Rigidbody, or
None for the world) and need no script. Bodies keep reacting to `AddForce`, `velocity` and so on while jointed.
