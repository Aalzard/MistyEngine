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
| WheelCollider | `motorTorque`, `brakeTorque`, `steerAngle`, `enabled` (settable), `rpm`, `isGrounded`, `radius`, `suspensionDistance` (read-only), `GetWorldPose()` (returns position and Euler angles), `GetGroundHit()` (WheelHit or nil) |
| SplineContainer | `EvaluatePosition(t [, spline])`, `EvaluateTangent(t)`, `EvaluateUpVector(t)` (world space, t is 0–1 by distance, spline index from 1), `EvaluateCurvature(t)`, `CalculateLength()`, `GetNearestPoint(position)` (point and t), `KnotCount()`, `GetKnotPosition(i)`, `splineCount`, `closed` |
| SplineAnimate | `Play()`, `Pause()`, `Restart()` (also lower-case), `normalizedTime` and `elapsedTime` (settable), `isPlaying`, `duration`, `speed`, `length`, `enabled` |
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
| `Input` | `GetKey/GetKeyDown/GetKeyUp("w")` (also `"mouse 0"`, `"joystick button 0"`, `"joystick 2 button 0"`), `GetMouseButton/Down/Up(0..2)`, `GetAxis(name)` (smoothed) / `GetAxisRaw(name)`, `GetButton/GetButtonDown/GetButtonUp(name)` (axes and buttons from Edit ▸ Project Settings ▸ Input Manager; an unknown name is an error), `GetJoystickNames()`, `anyKey`, `anyKeyDown`, `mousePosition`, `mouseScrollDelta` (notches) — keyboard and mouse need Game view focus (click it), gamepads always reach a playing game |
| `Debug` | `Log`, `LogWarning`, `LogError` (`print` = `Debug.Log`) |
| `Instantiate` | `Instantiate("Assets/Prefabs/Bullet.prefab" [, position])` or `Instantiate(gameObject [, position])` — returns the new "Name(Clone)" GameObject (at the root, with its physics and scripts; its `Awake` runs right after the current callback) |
| `SceneManager` | `LoadScene("Level2")` / `LoadScene(1)` — load a scene from File ▸ Build Settings by name, path or build index (after the current frame; in the editor until Stop); `GetActiveScene().name` |
| `Application` | `Quit()` — closes the built game (ignored with a Console message in the editor) |
| Tags | `gameObject.tag`, `gameObject:CompareTag("Enemy")`, `GameObject.FindWithTag(tag)` (first active object or nil), `GameObject.FindGameObjectsWithTag(tag)` (list); tags come from Project Settings ▸ Tags and Layers (an undefined tag is an error) |
| Other | `GameObject.Find(name)`, `Destroy(gameObject)`, `Mathf.*` (`Clamp`, `Lerp`, `Sin`, `PI`, `Infinity`, …), `ForceMode.Force/Impulse/VelocityChange/Acceleration` |

Key names: letters/digits (`"a"`, `"1"`), `"space"`, `"return"`, `"escape"`, `"tab"`, `"up"`, `"down"`, `"left"`,
`"right"`, `"left shift"`, `"left ctrl"`, `"left alt"`, `"f1"`…, mouse buttons `"mouse 0"`–`"mouse 2"` and joystick
buttons `"joystick button 0"` (any pad) / `"joystick 1 button 0"` (the first pad).

### Input Manager axes and gamepads

Edit ▸ Project Settings ▸ Input Manager lists the virtual axes. Default entries: Horizontal (←/→, A/D, left stick
X), Vertical (↓/↑, S/W, left stick Y), Fire1 (left ctrl, mouse 0, joystick button 0), Fire2 (left alt, mouse 1,
button 1), Fire3 (left shift, mouse 2, button 2), Jump (space, button 3), Mouse X, Mouse Y, Mouse ScrollWheel,
Submit (return, enter, space, button 0) and Cancel (escape, button 1). Several entries can share a name; the
one with the largest value wins.

- **Key or Mouse Button** axes move towards -1/0/+1 at Sensitivity units per second and fall back to 0 at Gravity
  per second (Snap jumps to 0 when the direction reverses; Invert swaps the sides). `GetAxisRaw` gives -1/0/+1.
  Smoothing uses real time (`Time.unscaledDeltaTime`).
- **Mouse Movement**: axis 0 = X, 1 = Y (up positive), 2 = wheel notches, times Sensitivity.
- **Joystick Axis**: the value of axis N of the chosen joystick (or the largest of all), 0 inside Dead, times
  Sensitivity.

Gamepads (via `gilrs`, in the player and in the editor while playing) use an Xbox-style layout. Buttons: 0 A,
1 B, 2 X, 3 Y, 4 left bumper, 5 right bumper, 6 back, 7 start, 8 left stick, 9 right stick, 10 guide, 11–14 d-pad
up/down/left/right. Axes (Axis field): X axis = left stick X, Y axis = left stick Y (+1 = down, so Vertical
inverts it), 3rd = triggers (left − right), 4th/5th = right stick X/Y, 6th/7th = d-pad X/Y (+1 = up), 9th/10th =
left/right trigger 0..1.

```lua
function Player:Update()
    local move = Vector3.new(Input.GetAxis("Horizontal"), 0, Input.GetAxis("Vertical"))
    self.cc:Move(move * (self.speed * Time.deltaTime))
    if Input.GetButtonDown("Jump") then self:Jump() end
end
```
Sample: `Scenes/InputDemo.scene` with `InputMover.lua` and `GamepadHint.lua`.

### Layers

`LayerMask.NameToLayer("Hidden")`, `LayerMask.LayerToName(8)` and `LayerMask.GetMask("Water", "Hidden")` use
the names from Project Settings ▸ Tags and Layers (layers 0–7 built in, 8–31 user layers). A camera's
`cullingMask` (from `Camera.main`) is a bit mask of the layers it draws: `cam.cullingMask = ~(1 << 8)` hides
layer 8.
`cam.useOcclusionCulling` (true by default) turns the camera's baked occlusion culling on or off (Window ▸
Rendering ▸ Occlusion Culling; see `Assets/Scripts/OcclusionTour.lua`).

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

### Loading addressable assets

```lua
Assets.Run(function()
    local h = Assets.InstantiateAsync("Enemies/Ghost", self.transform):Await()
    Debug.Log("spawned", h.result.name)
    Assets.Release(h)   -- destroys the instance, unloads unused bundles
end)
Assets.LoadAsync("props"):OnCompleted(function(h) Debug.Log(#h.results, "props") end)
```
Mark assets Addressable in the Inspector or the Asset Groups window; see `docs/asset_groups.md`.

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

### Wheel Collider

Add **Component ▸ Physics ▸ Wheel Collider** to a child of an object with a Rigidbody (one per wheel). Each
physics step the wheel casts a ray down its suspension (the object's position is the top; the wheel hangs up to
Suspension Distance below it), pushes the body up with the spring and damper (at rest it sits at the spring's
Target Position, 0 = fully compressed, 1 = fully extended), and its tyre grips along the Forward and Sideways
Friction curves. Wheels roll around their local X axis and drive along local Z; scale is ignored. Set the
**Wheel Model** to the visible tyre's object and it follows the wheel (steering, suspension and rolling).

```lua
local Drive = { torque = 900, steer = 30 }
function Drive:Start() self.fl = GameObject.Find("Wheel FL"):GetComponent("WheelCollider") --[[ … ]] end
function Drive:FixedUpdate()
    self.rl.motorTorque = Input.GetAxis("Vertical") * self.torque           -- N·m on the axle
    self.fl.steerAngle = Input.GetAxis("Horizontal") * self.steer             -- degrees
    self.rl.brakeTorque = Input.GetKey("space") and 6000 or 0
end
return Drive
```
`rpm`, `isGrounded` and `GetGroundHit()` describe the last physics step; the hit has `point`, `normal`, `force`
(suspension load in N), `forwardDir`, `sidewaysDir`, `forwardSlip` (negative while accelerating, positive while
braking), `sidewaysSlip`, `collider`/`gameObject`. `GetWorldPose()` returns the wheel centre and rotation (as
Euler angles: there is no Quaternion type) for moving your own models. Sample: `Scenes/VehicleDemo.scene` with
`Assets/Scripts/CarController.lua` (W/S throttle/brake, A/D steer, Space handbrake, R reset) and
`Assets/Scripts/CarFollowCamera.lua`.

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

## Testing (Test Runner)
Project tests are Lua files under `Assets/Tests/` (sub-folders too): `*.test.lua` are **EditMode**
tests and `*.playtest.lua` are **PlayMode** tests. Create them with Assets ▸ Create ▸ Testing ▸
EditMode Test / PlayMode Test and run them from Window ▸ General ▸ Test Runner (or the command
line, below).

```lua
describe("Inventory", function()
    local items
    beforeEach(function() items = {} end)

    it("starts empty", function()
        assert.equal(#items, 0)
    end)

    it.skip("is not written yet", function() end)
end)
```

| | |
|---|---|
| Declaring | `describe(name, fn)` (nests), `it(name, fn)` / `test(name, fn)`; `it.skip` / `xit`, `describe.skip` / `xdescribe`, `it(name)` without a function (pending); `it.only` / `describe.only` run just those tests of the file |
| Hooks | `beforeAll(fn)`, `afterAll(fn)`, `beforeEach(fn)`, `afterEach(fn)` at the top level or inside a `describe`; outer `beforeEach` first, inner `afterEach` first; `afterEach` runs even when the test or a `beforeEach` failed; a failing `beforeAll` fails every test of its block |
| `assert` | `equal(a, b [, msg])` (tables compared deeply), `notEqual`, `near(a, b [, eps [, msg]])` (numbers, Vector2/3, default eps 1e-6), `isTrue`, `isFalse`, `isNil`, `notNil`, `greater(a, b)`, `less(a, b)`, `contains(stringOrTable, x)`, `throws(fn [, textInMessage])` → the message, `fail(msg)`; `assert(v, msg)` still works |
| `expect(x)` | `.toBe(y)`, `.toEqual(y)`, `.toBeNear(y [, eps])`, `.toBeTruthy()`, `.toBeFalsy()`, `.toBeNil()`, `.toBeGreaterThan(n)`, `.toBeLessThan(n)`, `.toContain(x)`, `.toThrow([text])`, and `.never.toBe/toEqual/toBeNil/toContain`; `expect(x):toBe(y)` works too |

A failure reports the test file line that made it (click it in the Test Runner to open the Code
Editor there) and a Lua traceback. One failing test doesn't stop the others. Lua code that runs for
10 s without yielding (an endless loop) fails its test.

**EditMode** tests run with the game API against a scratch copy of the open scene (a fresh copy for
every test; nothing plays, no `Awake`/`Update`). `loadScene(name)` swaps the scratch scene for a
scene file. Edits never reach the real scene.

**PlayMode** tests each get their own play session, started on an empty scene and torn down after
the test; frames run at a fixed 1/60 s, so tests are deterministic. The test code is a coroutine
that uses the full game API on the running game:

| | |
|---|---|
| `loadScene(name)` | Restart the session on a scene (name in Build Settings, any `<name>.scene` under Assets, or a path); its `Awake` runs at once, `Start` on the next frame, then the test goes on |
| `yield(seconds)` | Let the game run for that much game time (at least one frame) |
| `yieldFrames(n)` | Let the game run `n` frames (1 by default) |
| `waitUntil(fn [, timeout])` | Run frames until `fn()` is true; fails after `timeout` s of game time (10) |
| `Input.simulateKeyDown(key)` / `simulateKeyUp(key)` | Hold / release a key from the next frame (`GetKey`, `GetKeyDown`, `GetAxis` see it) |
| `Input.simulateKeyPress(key)` | Press for one frame, released on the next |
| `Input.simulateMouseDown([button])` / `simulateMouseUp` / `simulateMousePosition(x, y)` | Mouse buttons and pointer (game-image pixels, 1280×720) |
| `Input.simulateReset()` | Release everything |
| `setTimeout(seconds)` | Longest game time the test may take (30 s by default) |
| `ignoreErrorLogs()` | Errors logged by the game or the test (`Debug.LogError`, script errors) fail a PlayMode or EditMode test unless it calls this |

```lua
describe("PlayerController2D", function()
    local player
    beforeEach(function()
        loadScene("Platformer2D")
        player = GameObject.Find("Player")
        yield(0.5)                       -- settle on the ground
    end)

    it("runs right while D is held", function()
        local start = player.transform.position.x
        Input.simulateKeyDown("d")
        yield(1)
        expect(player.transform.position.x):toBeGreaterThan(start + 3)
    end)
end)
```

Lua state (locals at the top of the file, upvalues) is shared by the tests of one file within a
run, like a test class; the scene and the play session are not. In PlayMode, `beforeAll` runs in the
first test's session, so objects it finds are gone in the next test; find them in `beforeEach`.

**Test Runner window**: EditMode / PlayMode tabs; a tree of files ▸ describe blocks ▸ tests with
pass/fail/skipped icons and durations; Run All, Run Selected (a file, a block or a test),
Run Failed, Cancel, Rerun on save; a search field; the selected test's message, file:line link,
traceback and output below; a summary bar that turns green or red. Tests run a slice per editor
frame, so the editor keeps responding (PlayMode tests step through game frames). Runs are refused
while the editor is in Play mode.

**Command line** (CI): `misty --run-tests <project> [--filter <text>] [--mode EditMode|PlayMode]
[--results results.xml]` runs every test (EditMode first) without a window against the first Build
Settings scene, prints PASS/FAIL lines, writes JUnit XML and exits with 0 (all passed), 1 (a test
failed) or 2 (bad arguments or project). `--filter` matches `file::Suite > test` case-insensitively.

Samples: `Assets/Tests/MathTests.test.lua`, `PlayerControllerTests.playtest.lua` (Platformer2D),
`VehicleTests.playtest.lua` (VehicleDemo).
