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
| `self.gameObject` | `name`, `tag`, `layer` (index, settable), `activeSelf`, `transform`, `SetActive(b)`, `CompareTag(t)`, `GetComponent("Rigidbody"/"Transform"/"AudioSource"/"Animation"/"Animator"/"CharacterController"/"NavMeshAgent"/…)` (nil if the object has none) |
| `self.transform` | `position`, `localPosition`, `eulerAngles`, `localEulerAngles`, `localScale`, `forward`, `right`, `up`, `parent`, `Translate(v)`, `Rotate(v)`, `LookAt(target)` |
| Rigidbody | `velocity`, `angularVelocity`, `mass`, `position` (physics pose, read-only), `isKinematic`, `useGravity`, `AddForce(v [, ForceMode.X])`, `AddTorque(v [, mode])`, `MovePosition(v)`, `MoveRotation(eulerDegrees)` |
| `Physics` | `Raycast`, `RaycastAll`, `SphereCast`, `OverlapSphere` (see Physics queries), `DefaultRaycastLayers`, `AllLayers`, `IgnoreRaycastLayer` |
| `LayerMask` | `GetMask("Default", "Player", …)`, `NameToLayer(name)` (-1 if unknown), `LayerToName(index)` |
| CharacterController | `Move(motion)`, `SimpleMove(speed)` (returns `isGrounded`), `isGrounded`, `velocity`, `height`, `radius`, `center`, `slopeLimit`, `stepOffset`, `skinWidth` (read-only) |
| AudioSource | `Play()`, `Stop()`, `Pause()`, `UnPause()`, `PlayOneShot(clipPath [, volumeScale])` (through the source's mix and Output group), `isPlaying` (read-only), `time` (seconds into the `Play()` clip; settable to seek), `volume`, `pitch`, `loop`, `mute`, `panStereo`, `spatialBlend`, `dopplerLevel`, `spread`, `reverbZoneMix`, `minDistance`, `maxDistance`, `rolloffMode` (`"Logarithmic"` / `"Linear"`), `priority` (0..256), `playOnAwake`, `bypassEffects`, `bypassListenerEffects`, `bypassReverbZones`, `enabled`, `clip` (project path like `"Assets/Audio/Beep.wav"`, or nil), `outputAudioMixerGroup` (a group, nil, or set by group name: looked up in the current Output's mixer first, then every project mixer) |
| `AudioSource` (global) | `PlayClipAtPoint(clipPath, position [, volume])` — a fully 3D one-shot at a fixed point (default rolloff, no mixer) |
| `AudioMixer` | `AudioMixer.Find("Main")` — the project's `.mixer` with that file name anywhere under `Assets/` (no Resources folder needed), or nil. Mixer: `name`, `SetFloat(exposedName, value)` (false if no such exposed parameter; volumes in dB), `GetFloat(name)` (current value or nil), `ClearFloat(name)` (back to the snapshot's value), `FindSnapshot(name)` (or nil), `FindMatchingGroups("SFX")` (array of groups whose path ends with it; `""` = all) |
| AudioMixerSnapshot | `name`, `audioMixer`, `TransitionTo(seconds)` (linear blend of every value; 0 jumps) |
| AudioMixerGroup | `name`, `audioMixer`; compares with `==`; assign to `outputAudioMixerGroup` |
| Animation | `Play([name])` (the default clip, or the clip of the Animation's list with that file name; restarts it; returns false and warns if missing), `Stop()` (rewinds, keeps the pose), `IsPlaying(name)`, `isPlaying` (read-only), `time` (seconds since Play, keeps growing while looping; settable), `speed` (1 = normal, negative plays backwards), `clip` (name of the current clip, read-only), `enabled` |
| Animator | `SetFloat(name, v)`, `GetFloat(name)`, `SetInteger`, `GetInteger`, `SetBool`, `GetBool`, `SetTrigger(name)`, `ResetTrigger(name)`, `Play(state [, layer [, normalizedTime]])`, `CrossFade(state, normalizedDuration [, layer [, normalizedOffset]])`, `CrossFadeInFixedTime(state, seconds [, …])`, `GetCurrentAnimatorStateInfo(0)` / `GetNextAnimatorStateInfo(0)` (table: `normalizedTime`, `length`, `speed`, `loop`, `IsName(name)`), `IsInTransition(0)`, `speed`, `enabled` (only layer 0; unknown names or wrong types warn in the Console) |
| MeshRenderer | `GetComponent("MeshRenderer")` / `("Renderer")`: `material`, `sharedMaterial`, `materials`, `sharedMaterials`, `enabled`, `castShadows`, `receiveShadows`, `gameObject` (see Materials below) |
| Material | `color`, `mainTextureOffset`, `mainTextureScale`, `shader`, `name`, `renderQueue`, `SetColor`/`GetColor`, `SetFloat`/`GetFloat`, `SetTexture`/`GetTexture`, `SetTextureOffset`/`SetTextureScale`, `HasProperty`, `EnableKeyword`/`DisableKeyword`/`IsKeywordEnabled` |
| Text (UI) | `GetComponent("Text")`: `text` (numbers are shown as text), `fontSize`, `color`, `enabled`, `gameObject` |
| Image (UI) | `GetComponent("Image")`: `color`, `texture` (project path or nil), `raycastTarget`, `enabled`, `gameObject` |
| Button (UI) | `GetComponent("Button")`: `interactable`, `gameObject`, `onClick`; clicks call the On Click methods set in the Inspector |
| Toggle (UI) | `GetComponent("Toggle")`: `isOn`, `interactable`, `group` (ToggleGroup or nil), `onValueChanged` (bool) |
| ToggleGroup (UI) | `allowSwitchOff`, `AnyTogglesOn()`, `SetAllTogglesOff()` |
| Slider (UI) | `value` (clamped, rounded with whole numbers), `minValue`, `maxValue`, `wholeNumbers`, `normalizedValue`, `interactable`, `onValueChanged` (number) |
| Scrollbar (UI) | `value` (0..1), `size`, `numberOfSteps`, `interactable`, `onValueChanged` (number) |
| InputField (UI) | `text` (filtered by the content type and limit), `characterLimit`, `contentType` (`"Standard"`, `"IntegerNumber"`, `"DecimalNumber"`, `"Password"`), `readOnly`, `interactable`, `onValueChanged` / `onEndEdit` (string) |
| Dropdown (UI) | `value` (0-based index), `options` (array of strings; read or assign), `captionText`, `AddOptions({…})`, `ClearOptions()`, `RefreshShownValue()`, `interactable`, `onValueChanged` (integer) |
| ScrollRect (UI) | `horizontal`, `vertical`, `enabled`, `content` (GameObject) |
| RawImage (UI) | `color`, `texture`, `enabled`, `gameObject` |
| `Color` | `new(r,g,b [,a])` (0..1), `white black red green blue yellow cyan magenta gray clear`, `Lerp(a,b,t)`; `.r .g .b .a`, `* number/Color`, `==` |
| `Vector3` | `new(x,y,z)`, `zero one up down left right forward back`, `Distance`, `Dot`, `Cross`, `Lerp`; `+ - * /`, `.magnitude`, `.normalized` |
| `Time` | `deltaTime`, `time`, `frameCount`, `fixedDeltaTime`, `timeScale`, `unscaledDeltaTime` (read-only; `fixedDeltaTime` and `timeScale` come from Edit ▸ Project Settings) |
| `Input` | `GetKey/GetKeyDown/GetKeyUp("w")`, `GetMouseButton(0)`, `mousePosition`, `GetAxis("Horizontal"/"Vertical"/"Mouse X"/"Mouse Y")` — the Game view must have focus (click it) |
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

### Driving an Animator

```lua
local Hero = {}
function Hero:Start() self._anim = self.gameObject:GetComponent("Animator") end
function Hero:Update()
    if Input.GetKeyDown("space") then self._anim:SetTrigger("Jump") end
    self._anim:SetFloat("Speed", Input.GetAxis("Vertical"))
    local info = self._anim:GetCurrentAnimatorStateInfo(0)
    if info:IsName("Jump") and info.normalizedTime > 0.5 then Debug.Log("coming down") end
end
return Hero
```
Parameters set in `Update` are used the same frame: the Animator is evaluated after `Update` (after the Animation
component), before `LateUpdate`. A trigger stays set until a transition uses it (or `ResetTrigger`).
`normalizedTime` keeps growing while a state loops (1.5 = halfway through the second loop). Sample:
`Assets/Scripts/AnimatorControl.lua` in `Scenes/AnimatorDemo.scene` (Space jumps, 1/2/3 set the speed, 4 dances).

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

### UI events and listeners

Widgets report changes in two ways, like the familiar UnityEvents:

- **In the Inspector:** On Value Changed / On End Edit lists (Toggle, Slider, Scrollbar, Input Field, Dropdown)
  work like On Click; the method gets the new value (`true`/`false`, a number, the text or the option index).
- **From Lua:** `onClick`, `onValueChanged` and `onEndEdit` have `AddListener(fn)`, `RemoveListener(fn)` and
  `RemoveAllListeners()`.

```lua
function SettingsMenu:Start()
    local volume = GameObject.Find("Volume Slider"):GetComponent("Slider")
    volume.onValueChanged:AddListener(function(v) Debug.Log("volume", v) end)
    GameObject.Find("Level 1"):GetComponent("Button").onClick:AddListener(function() self:Play(1) end)
end
```
Events run in the UI step of the frame (before `FixedUpdate`/`Update`). A value changed by a script (for example
`slider.value = 3`) is reported in the next frame's UI step rather than inside the assignment; a widget created
this frame starts reporting from the next one. Sample: `Assets/Scenes/UIWidgetsDemo.scene` with
`Assets/Scripts/SettingsMenu.lua`.

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

### Render settings

The global `RenderSettings` reads and changes the open scene's Lighting window values; changes show on the
next frame.

| Member | Meaning |
| --- | --- |
| `fog` | Fog on/off (boolean). |
| `fogColor` | Fog colour (`Color`; alpha is ignored). |
| `fogDensity` | Density for the Exponential / Exponential Squared modes. |
| `fogStartDistance`, `fogEndDistance` | Linear fog range. |
| `ambientIntensity` | Environment Lighting intensity multiplier. |
| `reflectionIntensity` | Environment Reflections intensity multiplier. |

```lua
local Dusk = { speed = 0.02 }
function Dusk:Start() RenderSettings.fog = true end
function Dusk:Update()
    RenderSettings.fogDensity = Mathf.Min(RenderSettings.fogDensity + self.speed * Time.deltaTime, 0.1)
    RenderSettings.fogColor = Color.new(0.8, 0.55, 0.4)
end
return Dusk
```
The fog mode and the skybox are not scriptable yet.

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

### Terrain

`Terrain.activeTerrain` is the first enabled terrain in the scene (`nil` without one); `Terrain.activeTerrains`
lists them all; `GetComponent("Terrain")` returns an object's terrain.

| Member | Meaning |
| --- | --- |
| `SampleHeight(position)` | Height of the surface under a world position, relative to the terrain's position (points outside use the nearest edge). |
| `GetPosition()` | World position of the terrain's corner. |
| `terrainData` | Table with `size` (Vector3: width, height, length), `heightmapResolution`, `alphamapResolution`, `layerCount`. |
| `gameObject`, `transform` | The terrain's object. |

```lua
-- Keep an object 1.5 m above the ground.
local t = Terrain.activeTerrain
local p = self.transform.position
p.y = t:SampleHeight(p) + t:GetPosition().y + 1.5
self.transform.position = p
```
Sample: `Scenes/TerrainDemo.scene` with `Assets/Scripts/TerrainHover.lua`.

### Materials

`GetComponent("MeshRenderer")` (or `"Renderer"`) gives the renderer's materials:

| Member | Meaning |
| --- | --- |
| `material` | This object's own copy of Element 0, made on first use; changing it changes only this object. Assigning a material sets the element (an asset by path, or a copy of another instance). |
| `sharedMaterial` | The material asset itself: changes show on every object using it (and on later `Instantiate`s). Once the renderer has its own copy, `sharedMaterial` is that copy. |
| `materials` / `sharedMaterials` | Lists (1-based) of all Materials elements, as copies / as assets; assign a list to resize. |

Changes made from scripts last until Play mode ends: the `.mat` files are never written.
A Material has `color`, `mainTextureOffset` / `mainTextureScale` (Vector3, z ignored, or `{ x =, y = }`),
`shader` (`"Standard"`, `"Unlit/Color"`, `"Unlit/Texture"`, `"Unlit/Transparent"`), `name` and `renderQueue`
(read-only), and the Standard shader's property names:

| Kind | Names |
| --- | --- |
| `SetColor` / `GetColor` | `_Color` (albedo), `_EmissionColor` (HDR: components above 1 raise the intensity) |
| `SetFloat` / `GetFloat` | `_Metallic`, `_Glossiness` / `_GlossMapScale` (smoothness), `_BumpScale`, `_Cutoff`, `_OcclusionStrength`, `_Mode` (0 Opaque, 1 Cutout, 2 Fade, 3 Transparent), `_SmoothnessTextureChannel` (0 metallic alpha, 1 albedo alpha), `_Cull` (0 = double sided, 2 = back) |
| `SetTexture` / `GetTexture` | `_MainTex`, `_BumpMap`, `_MetallicGlossMap`, `_OcclusionMap`, `_EmissionMap` (project paths or nil) |
| Keywords | `EnableKeyword("_EMISSION")` / `DisableKeyword` switch emission; `IsKeywordEnabled` also answers `_NORMALMAP`, `_METALLICGLOSSMAP`, `_ALPHATEST_ON`, `_ALPHABLEND_ON`, `_ALPHAPREMULTIPLY_ON` |

Unknown property names are ignored (`Get…` returns 0 / black, `HasProperty` false).

```lua
-- Pulse this object's colour without touching other objects using the same material.
local r = self.gameObject:GetComponent("MeshRenderer")
local t = (math.sin(Time.time * 2) + 1) * 0.5
r.material.color = Color.new(1, t, 0, 1)
r.material:SetColor("_EmissionColor", Color.new(2 * t, t, 0, 1))
```
Sample: `Scenes/MaterialsDemo.scene` with `Assets/Scripts/MaterialPulse.lua`.

### Navigation

Bake a nav mesh first (Window ▸ AI ▸ Navigation ▸ Bake; floors and walls need the Static flag). Objects with a
Nav Mesh Agent then walk paths on it: `GetComponent("NavMeshAgent")`.

| Member | Meaning |
| --- | --- |
| `SetDestination(v)` | Plan a path to the nav mesh point nearest `v`; false when the agent isn't on the mesh. |
| `destination` | Where the current path ends (setting it calls `SetDestination`). |
| `remainingDistance` | Distance left along the path (0 without a path). |
| `hasPath`, `pathPending`, `pathStatus` | Path state. Paths are computed at once, so `pathPending` is always false; `pathStatus` is `"PathComplete"`, `"PathPartial"` (target unreachable: the path ends as close as possible) or `"PathInvalid"`. |
| `isStopped` | true = brake and stay, keeping the path. |
| `velocity` | Current velocity (settable). |
| `isOnNavMesh`, `nextPosition`, `steeringTarget` | Placed on the mesh, position on the mesh, next corner. |
| `speed`, `angularSpeed`, `acceleration`, `stoppingDistance`, `autoBraking`, `radius`, `height`, `baseOffset`, `avoidancePriority`, `enabled` | The component's settings. |
| `ResetPath()` | Drop the path. |
| `Warp(position)` | Jump to the nav mesh point nearest `position` (false if none is close). |

An agent arriving with a Stopping Distance of 0 drops its path (`hasPath` false, `remainingDistance` 0).
Moving an agent's Transform from a script places the agent there.

```lua
-- Patrol between two points.
function Guard:Update()
    if not self.agent.pathPending and self.agent.remainingDistance < 0.3 then
        self.toA = not self.toA
        self.agent:SetDestination(self.toA and self.a or self.b)
    end
end
```

`NavMesh.SamplePosition(position, maxDistance)` returns `{ position, distance, hit = true }` for the nearest nav
mesh point within `maxDistance`, or nil. `NavMesh.CalculatePath(from, to)` returns the path's corners (a list of
Vector3, so `#path` is the corner count) with `path.status`. Area-mask arguments are accepted and ignored.

### Camera and mouse

`Camera.main` is the first enabled camera tagged MainCamera (nil without one): `ScreenPointToRay(screenPoint)`
returns `{ origin, direction }`, `WorldToScreenPoint(v)` returns pixels (from the bottom-left) and the depth.
`Input.mousePosition` is the pointer in Game view pixels from the bottom-left (z = 0).

```lua
-- Click to move.
if Input.GetMouseButtonDown(0) then
    local ray = Camera.main:ScreenPointToRay(Input.mousePosition)
    local hit = Physics.Raycast(ray.origin, ray.direction, 200)
    if hit ~= nil then
        self.agent:SetDestination(hit.point)
    end
end
```
Sample: `Scenes/NavigationDemo.scene` with `Patrol.lua`, `Chaser.lua` and `ClickMarker.lua`.

### 2D physics and sprites
2D values are Vector3 with z = 0: `Vector2.new(x, y)` / `Vector2(x, y)`, `Vector2.zero/one/up/down/left/right`,
`Vector2.Distance/Dot/Lerp`; 2D functions also take `{x=, y=}` tables or two numbers.

| | |
|---|---|
| Rigidbody2D | `GetComponent("Rigidbody2D")`: `velocity` (settable), `angularVelocity` (degrees/s), `position`, `rotation` (read-only), `mass`, `gravityScale`, `bodyType` (`"Dynamic"`/`"Kinematic"`/`"Static"`), `isKinematic`, `freezeRotation`, `AddForce(v [, ForceMode2D.Force/Impulse])`, `AddTorque(t [, mode])`, `MovePosition(v)`, `MoveRotation(degrees)` |
| 2D colliders | `GetComponent("BoxCollider2D"/"CircleCollider2D"/"CapsuleCollider2D"/"PolygonCollider2D"/"EdgeCollider2D")`: `isTrigger`, `enabled`, `offset`, `size`, `radius`, `attachedRigidbody`, `CompareTag` (read-only) |
| SpriteRenderer | `sprite` (`"Assets/Sprites/Tiles.png#Tiles_1"`), `color`, `flipX`, `flipY`, `sortingLayerName`, `sortingOrder`, `enabled` |
| `Physics2D` | `Raycast(origin, dir [, distance [, mask]])` → hit table (`point`, `normal`, `distance`, `fraction`, `gameObject`, `collider`, `transform`, `rigidbody`) or nil, `RaycastAll`, `OverlapCircle(c, r [, mask])` / `OverlapBox(c, size [, angle [, mask]])` / `OverlapPoint(p [, mask])` → GameObject or nil, `OverlapCircleAll` → list, `gravity` (read/write) |
| Image (UI) | also `sprite` (sprite reference or nil) |

Callbacks: `OnCollisionEnter2D(c)`, `OnCollisionStay2D(c)`, `OnCollisionExit2D(c)` with `c.gameObject`, `c.collider`,
`c.otherCollider`, `c.rigidbody`, `c.relativeVelocity`, `c.contacts[1].point/normal`, `c:GetContact(0)`;
`OnTriggerEnter2D(other)`, `OnTriggerStay2D(other)`, `OnTriggerExit2D(other)` with the other GameObject.
Rays that start inside a collider report it (Project Settings ▸ Physics 2D ▸ Queries Start In Colliders), so
ground checks use a layer mask:

```lua
local grounded = Physics2D.Raycast(self.transform.position + Vector2.down * 0.5, Vector2.down, 0.1,
    LayerMask.GetMask("Default")) ~= nil
```

Sample: `Assets/Scripts/PlayerController2D.lua`, `Assets/Scripts/CameraFollow2D.lua` (Platformer2D scene).
