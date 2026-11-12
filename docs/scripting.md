# Scripting (Lua)

Scripts are `.lua` files in `Assets/`. Create one with **Asset Files ▸ + ▸ Script** (or Add Archetype ▸ New script),
attach it by dragging it onto an object (Editor Scene, Scene Tree, or Properties ▸ Add Archetype ▸ Scripts), and
double-click it to edit. Changes are picked up automatically; syntax errors appear in the Console.

Prefer nodes to code? [Script Graphs](visual-scripting.md) attach the same way and talk to Lua scripts
(custom events, `gameObject:GetVariable`, Call Lua Function).

```lua
local Rotator = {
    speed = 90,                    -- public fields: number, boolean, string, Vector3
    axis = Vector3.new(0, 1, 0),   -- shown in the Properties panel as "Speed", "Axis"
}

function Rotator:Update()
    self.transform:Rotate(self.axis * (self.speed * Time.deltaTime))
end

return Rotator                     -- a script must return its class table
```

Game scripts run in a sandbox: Lua's `table`, `string`, `math`, `utf8` and `coroutine`
libraries and `os.time`, `os.clock`, `os.date`, `os.difftime`, but no file, process or module
access (`io`, `os.execute`, `require`, `dofile`), and `load` takes source text only. Paths
given to the API (`Instantiate("Assets/…")`, clips, input actions) must be project-relative.
The scripts of one frame may run for at most 5 seconds; past that the running script stops
with an error (an endless loop no longer freezes the editor). Editor scripts are not
sandboxed.

## Callbacks
`Awake`, `Start`, `Update`, `FixedUpdate` (every 0.02 s physics step), `LateUpdate`,
`OnCollisionEnter(other)`, `OnCollisionExit(other)`, `OnTriggerEnter(other)`, `OnTriggerExit(other)`
(`other` is a GameObject), `OnControllerColliderHit(hit)` (see Character Controller below),
`OnAnimatorMove()` (root motion, see Driving an Animator below). Fields starting
with `_` stay private.
Animation events call a function by name (no arguments) on the scripts of the object with the Animation.
Save games call `OnSave(data)` / `OnLoad(data)` and locale switches `OnLocaleChanged(code)` (see Save games
and Localization below).
Scripts on network objects get `OnNetworkSpawn`, `OnNetworkDespawn` and the ownership callbacks (see
Networking below).

## Field attributes
List attributes per field in the class's `_attributes` table to change how the Properties panel shows it:

```lua
local Satellite = { center = Vector3.new(0, 1, 0), radius = 3, angle = 0 }
Satellite._attributes = {
    center = { Header("Orbit"), Tooltip("World point the object circles around") },
    radius = { Range(0.5, 20) },          -- a slider instead of a number field
    angle = { HideInInspector() },        -- still saved and used, just not shown
}
```

`Header(text)` (bold heading above), `Tooltip(text)` (hover text of the label), `Space([pixels])` (gap
above, 8 by default), `Range(min, max)` (numbers) and `HideInInspector()`. One attribute can stand on its
own (`radius = Range(0, 1)`). The functions exist in games too and cost nothing there.

## API
| | |
|---|---|
| `self.gameObject` | `name`, `tag`, `layer` (index, settable), `activeSelf`, `transform`, `SetActive(b)`, `CompareTag(t)`, `GetComponent("Rigidbody"/"Transform"/"AudioSource"/"Animation"/"Animator"/"Camera"/"CharacterController"/"NavMeshAgent"/…)` (nil if the object has none); `GetComponent("Rotator")` is that object's `Rotator.lua` instance (its fields and functions), nil without one |
| `self.transform` | `position`, `localPosition`, `eulerAngles`, `localEulerAngles`, `localScale`, `forward`, `right`, `up`, `parent`, `childCount`, `GetChild(i)` (0-based), `Find("Head/Camera")` (a child or a path of children, or nil), `Translate(v)`, `Rotate(v)`, `LookAt(target)` |
| Camera | `GetComponent("Camera")` or `Camera.main`: `fieldOfView` (vertical degrees), `nearClipPlane`, `farClipPlane`, `orthographic`, `orthographicSize`, `enabled`, `cullingMask`, `useOcclusionCulling`, `ScreenPointToRay`, `WorldToScreenPoint` (see Camera and mouse) |
| Rigidbody | `velocity`, `angularVelocity`, `mass`, `position` (physics pose, read-only), `isKinematic`, `useGravity`, `AddForce(v [, ForceMode.X])`, `AddTorque(v [, mode])`, `MovePosition(v)`, `MoveRotation(eulerDegrees)` |
| `Physics` | `Raycast`, `RaycastAll`, `SphereCast`, `OverlapSphere` (see Physics queries), `DefaultRaycastLayers`, `AllLayers`, `IgnoreRaycastLayer` |
| `LayerMask` | `GetMask("Default", "Player", …)`, `NameToLayer(name)` (-1 if unknown), `LayerToName(index)` |
| CharacterController | `Move(motion)`, `SimpleMove(speed)` (returns `isGrounded`), `isGrounded`, `velocity`, `height`, `radius`, `center` (settable: the capsule is rebuilt, e.g. crouching), `slopeLimit`, `stepOffset`, `skinWidth` (read-only) |
| WheelCollider | `motorTorque`, `brakeTorque`, `steerAngle`, `enabled` (settable), `rpm`, `isGrounded`, `radius`, `suspensionDistance` (read-only), `GetWorldPose()` (returns position and Euler angles), `GetGroundHit()` (WheelHit or nil) |
| SplineContainer | `EvaluatePosition(t [, spline])`, `EvaluateTangent(t)`, `EvaluateUpVector(t)` (world space, t is 0–1 by distance, spline index from 1), `EvaluateCurvature(t)`, `CalculateLength()`, `GetNearestPoint(position)` (point and t), `KnotCount()`, `GetKnotPosition(i)`, `splineCount`, `closed` |
| SplineAnimate | `Play()`, `Pause()`, `Restart()` (also lower-case), `normalizedTime` and `elapsedTime` (settable), `isPlaying`, `duration`, `speed`, `length`, `enabled` |
| AudioSource | `Play()`, `Stop()`, `Pause()`, `UnPause()`, `PlayOneShot(clipPath [, volumeScale])` (through the source's mix and Output group), `isPlaying` (read-only), `time` (seconds into the `Play()` clip; settable to seek), `volume`, `pitch`, `loop`, `mute`, `panStereo`, `spatialBlend`, `dopplerLevel`, `spread`, `reverbZoneMix`, `minDistance`, `maxDistance`, `rolloffMode` (`"Logarithmic"` / `"Linear"` / `"Custom"`), `spatialize` (binaural head model for the 3D part), `SetCustomCurve("CustomRolloff", {{distance, volume}, ...})` (distance as a share of Max Distance, 0..1; also switches to Custom) and `GetCustomCurve("CustomRolloff")`, `priority` (0..256), `playOnAwake`, `bypassEffects`, `bypassListenerEffects`, `bypassReverbZones`, `enabled`, `clip` (project path like `"Assets/Audio/Beep.wav"`, or nil), `outputAudioMixerGroup` (a group, nil, or set by group name: looked up in the current Output's mixer first, then every project mixer) |
| `AudioSource` (global) | `PlayClipAtPoint(clipPath, position [, volume])` — a fully 3D one-shot at a fixed point (default rolloff, no mixer) |
| `AudioMixer` | `AudioMixer.Find("Main")` — the project's `.mixer` with that file name anywhere under `Assets/` (no Resources folder needed), or nil. Mixer: `name`, `SetFloat(exposedName, value)` (false if no such exposed parameter; volumes in dB; any effect parameter, Send level or wet mix level can be exposed), `GetFloat(name)` (current value or nil), `ClearFloat(name)` (back to the snapshot's value), `FindSnapshot(name)` (or nil), `TransitionToSnapshots({snapA, snapB}, {weightA, weightB}, seconds)` (move to the weighted blend Σwᵢvᵢ / Σwᵢ of several snapshots), `outputAudioMixerGroup` (a group of another mixer this one plays into: nested mixers; nil = straight out), `FindMatchingGroups("SFX")` (array of groups whose path ends with it; `""` = all) |
| AudioMixerSnapshot | `name`, `audioMixer`, `TransitionTo(seconds)` (linear blend of every value; 0 jumps) |
| AudioMixerGroup | `name`, `audioMixer`; compares with `==`; assign to `outputAudioMixerGroup` |
| Animation | `Play([name])` (the default clip, or the clip of the Animation's list with that file name; restarts it; returns false and warns if missing), `Stop()` (rewinds, keeps the pose), `IsPlaying(name)`, `isPlaying` (read-only), `time` (seconds since Play, keeps growing while looping; settable), `speed` (1 = normal, negative plays backwards), `clip` (name of the current clip, read-only), `enabled` |
| Animator | `SetFloat(name, v [, dampTime, deltaTime])`, `GetFloat(name)`, `SetInteger`, `GetInteger`, `SetBool`, `GetBool`, `SetTrigger(name)`, `ResetTrigger(name)`, `Play(state [, layer [, normalizedTime]])`, `CrossFade(state, normalizedDuration [, layer [, normalizedOffset]])`, `CrossFadeInFixedTime(state, seconds [, …])` (layer -1 or none = the first layer with that state; states can be named "Layer.Machine.State"), `GetCurrentAnimatorStateInfo(layer)` / `GetNextAnimatorStateInfo(layer)` (table: `name`, `fullPath`, `tag`, `normalizedTime`, `length`, `speed`, `loop`, `IsName(name)`, `IsTag(tag)`), `IsInTransition(layer)`, `layerCount`, `GetLayerName(i)`, `GetLayerIndex(name)`, `GetLayerWeight(i)`, `SetLayerWeight(i, w)`, `applyRootMotion`, `deltaPosition`, `deltaRotation` (Euler degrees), `velocity`, `ApplyBuiltinRootMotion()`, `MatchTarget(position, eulerRotation, "Root", {positionXYZWeight=…, rotationWeight=…}, start [, target])`, `InterruptMatchTarget([complete])`, `isMatchingTarget`, `speed`, `enabled` (unknown names or wrong types warn in the Console; a layer index out of range is an error) |
| MeshRenderer | `GetComponent("MeshRenderer")` / `("Renderer")`: `material`, `sharedMaterial`, `materials`, `sharedMaterials`, `enabled`, `castShadows`, `receiveShadows`, `gameObject` (see Materials below) |
| Material | `color`, `mainTextureOffset`, `mainTextureScale`, `shader`, `name`, `renderQueue`, `SetColor`/`GetColor`, `SetFloat`/`GetFloat`, `SetTexture`/`GetTexture`, `SetTextureOffset`/`SetTextureScale`, `HasProperty`, `EnableKeyword`/`DisableKeyword`/`IsKeywordEnabled` |
| Text (UI) | `GetComponent("Text")`: `text` (numbers are shown as text), `fontSize`, `color`, `enabled`, `gameObject` |
| Image (UI) | `GetComponent("Image")`: `color`, `texture` (project path or nil), `raycastTarget`, `enabled`, `gameObject`, `type` (`"Simple"`, `"Sliced"`, `"Tiled"`, `"Filled"`), `fillAmount` (0..1), `fillMethod` (`"Horizontal"`, `"Vertical"`, `"Radial90"`, `"Radial180"`, `"Radial360"`), `fillOrigin` (index of the method's origins), `fillClockwise`, `fillCenter`, `preserveAspect` |
| Canvas (UI) | `GetComponent("Canvas")`: `renderMode` (`"ScreenSpaceOverlay"`, `"ScreenSpaceCamera"`, `"WorldSpace"`), `sortingOrder`, `planeDistance`, `enabled`, `gameObject` |
| CanvasGroup (UI) | `GetComponent("CanvasGroup")`: `alpha`, `interactable`, `blocksRaycasts`, `ignoreParentGroups`, `enabled`, `gameObject` |
| AspectRatioFitter (UI) | `GetComponent("AspectRatioFitter")`: `aspectMode` (`"None"`, `"WidthControlsHeight"`, `"HeightControlsWidth"`, `"FitInParent"`, `"EnvelopeParent"`), `aspectRatio`, `gameObject` |
| Button (UI) | `GetComponent("Button")`: `interactable`, `gameObject`, `onClick`; clicks call the On Click methods set in the Properties panel |
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
| `Input` | `GetKey/GetKeyDown/GetKeyUp("w")` (also `"mouse 0"`, `"joystick button 0"`, `"joystick 2 button 0"`), `GetMouseButton/Down/Up(0..2)`, `GetAxis(name)` (smoothed) / `GetAxisRaw(name)`, `GetButton/GetButtonDown/GetButtonUp(name)` (axes and buttons from Edit ▸ Project Settings ▸ Input Manager; an unknown name is an error), `GetJoystickNames()`, `anyKey`, `anyKeyDown`, `mousePosition`, `mouseDelta` (pixels moved this frame, y up; raw motion while the cursor is locked), `mouseScrollDelta` (notches) — keyboard and mouse need Scene Viewer focus (click it), gamepads always reach a playing game |
| `Cursor` | `lockState` (`CursorLockMode.None` / `Locked` / `Confined`), `visible` — locking hides the cursor and holds it in the window so mouse look reads raw motion; in the editor Escape always gives it back (scripts then see `None`), Stop and a lost focus too |
| `Debug` | `Log`, `LogWarning`, `LogError` (`print` = `Debug.Log`) |
| `Instantiate` | `Instantiate("Assets/Prefabs/Bullet.prefab" [, position])` or `Instantiate(gameObject [, position])` — returns the new "Name(Clone)" GameObject (at the root, with its physics and scripts; its `Awake` runs right after the current callback) |
| `SceneManager` | `LoadScene("Level2")` / `LoadScene(1)` — load a scene from File ▸ Build Settings by name, path or build index (after the current frame; in the editor until Stop); `GetActiveScene().name` |
| `Application` | `Quit()` — closes the built game (ignored with a Console message in the editor); `HasDefine(name)` — the Build Profile's scripting defines; `isDedicatedServer`, `isEditor` |
| `Achievements` | `Unlock(id)` (true the first time), `IsUnlocked(id)`, `Clear(id)`, `GetUnlocked()` — stored locally (see docs/building.md) |
| Tags | `gameObject.tag`, `gameObject:CompareTag("Enemy")`, `GameObject.FindWithTag(tag)` (first active object or nil), `GameObject.FindGameObjectsWithTag(tag)` (list); tags come from Project Settings ▸ Tags and Layers (an undefined tag is an error) |
| Other | `GameObject.Find(name)`, `Destroy(gameObject)`, `Mathf.*` (`Clamp`, `Lerp`, `Sin`, `PI`, `Infinity`, …), `ForceMode.Force/Impulse/VelocityChange/Acceleration` |

Key names: letters/digits (`"a"`, `"1"`), `"space"`, `"return"`, `"escape"`, `"tab"`, `"up"`, `"down"`, `"left"`,
`"right"`, `"left shift"`, `"left ctrl"`, `"left alt"`, `"f1"`…, mouse buttons `"mouse 0"`–`"mouse 2"` and joystick
buttons `"joystick button 0"` (any pad) / `"joystick 1 button 0"` (the first pad).

### Input Manager axes and gamepads

Edit ▸ Project Settings ▸ Input Manager lists the virtual axes. Default entries: Horizontal (←/→, A/D, left stick
X), Vertical (↓/↑, S/W, left stick Y), Fire1 (left ctrl, mouse 0, joystick button 0), Fire2 (left alt, mouse 1,
button 1), Fire3 (left shift, mouse 2, button 2), Jump (space, button 3), Run (left/right shift, left stick
press), Crouch (left ctrl, C, button 1), Mouse X, Mouse Y, Mouse ScrollWheel, Submit (return, enter, space,
button 0) and Cancel (escape, button 1). Several entries can share a name; the one with the largest value wins.

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

### Input System (actions)

Action-based input next to the Input Manager. An **Input Actions** asset (`.inputactions`, Assets ▸ Create ▸
Input Actions, edited in Window ▸ Input ▸ Input Actions or by double-clicking it) holds **action maps** (Player,
UI, …) of **actions**. Each action has a type and **bindings**:

- **Value** (the most actuated control drives it; started + performed when it leaves its default, performed on
  every change, canceled back at the default), **Button** (performed past the press point 0.5, canceled on
  release), **Pass Through** (every change of every control is performed). The control type (Any, Button,
  Axis, Vector 2) sets what `ReadValue` returns.
- **Binding paths**: `<Keyboard>/w`, `<Keyboard>/leftShift`, `<Keyboard>/shift` (either), `<Mouse>/delta`
  (pixels, y up), `<Mouse>/scroll` (notches), `<Mouse>/position`, `<Mouse>/leftButton`,
  `<Gamepad>/leftStick` (also `/x`, `/y`, `/up` …), `<Gamepad>/buttonSouth` (`a`/`cross` aliases),
  `<Gamepad>/leftTrigger`, `<Gamepad>/dpad`, `<Gamepad>/start`, `<Touchscreen>/primaryTouch/position`,
  `<Touchscreen>/primaryTouch/press`, `<Pointer>/position`.
- **Composites**: `2DVector` (up/down/left/right, modes DigitalNormalized, Digital, Analog: WASD),
  `1DAxis` (negative/positive, `whichSideWins`, `minValue`, `maxValue`), `ButtonWithOneModifier`
  (Shift + B), `ButtonWithTwoModifiers`, `OneModifier` (modifier + any binding).
- **Processors** (on a binding or an action, left to right): `invert`, `invertVector2(invertX,invertY)`,
  `normalize(min,max,zero)`, `normalizeVector2`, `scale(factor)`, `scaleVector2(x,y)`, `clamp(min,max)`,
  `axisDeadzone(min,max)`, `stickDeadzone(min,max)` (defaults 0.125 / 0.925).
- **Interactions**: `press(behavior=PressOnly|ReleaseOnly|PressAndRelease)`, `hold(duration=0.4)`,
  `tap(duration=0.2)`, `slowTap(duration=0.5)`, `multiTap(tapCount=2,tapTime=0.2,tapDelay=0.75)`, all with
  `pressPoint`. With several, the first to start drives the action; when it cancels another one that started
  takes over (`hold,tap` on one key: a quick press taps, a long one holds).
- **Control schemes** (Keyboard&Mouse, Gamepad, Touch) list the devices they need; bindings join them through
  their groups ("Use in control scheme").

The **Player Input** component (Archetype ▸ Input) plays an asset for one player: Actions, Default Map, Default
Scheme, Auto-Switch and a Behavior:

- **Send Messages** calls `On<Action>(value)` on the object's scripts (`OnMove(v)`, `OnJump()`): on performed,
  and on canceled for Value / Pass Through actions (with the zero value). Also `OnControlsChanged(player)`,
  `OnDeviceLost(player)`, `OnDeviceRegained(player)`. **Broadcast Messages** does the same on the children.
- **Invoke Events** calls the methods listed under Events (per action) with a callback context on started,
  performed and canceled.

A single player may use every device; its binding mask follows the current control scheme, which switches when
another device is used (Auto-Switch). With several Player Inputs each one is paired with its own devices in
Scene Tree order (the first gets the keyboard and mouse, the next ones a gamepad each), gets its own copy of the
asset, and is told when its pad is unplugged and comes back.

```lua
local Player = {}
function Player:Start()
    local pi = self.gameObject:GetComponent("PlayerInput")
    self.move = pi.actions["Move"]            -- or pi.actions:FindAction("Player/Move")
    self.fire = pi.actions:FindAction("Attack")
    self.fire.performed:AddListener(function(ctx) self:Shoot(ctx:ReadValue()) end)
    pi.onControlsChanged:AddListener(function(p) Debug.Log(p.currentControlScheme) end)
end
function Player:Update()
    local v = self.move:ReadValue()               -- Vector2
    self.cc:Move(Vector3.new(v.x, 0, v.y) * (5 * Time.deltaTime))
end
function Player:OnJump() self:Jump() end          -- Send Messages
return Player
```

| API | |
|---|---|
| Assets | `InputActions.Load(path)` (the shared copy; Player Input's first player uses it too), `InputActions.Instantiate(path)` (a separate copy); `actions:FindAction("Map/Action" or "Action")`, `actions["Move"]`, `actions:FindActionMap(name)`, `actions:Enable()` / `Disable()`, `actionMaps`, `controlSchemes`, `bindingMask` |
| Maps | `map:Enable()` / `Disable()`, `map.enabled`, `map.name`, `map.actions`, `map:FindAction(name)`, `map["Jump"]` |
| Actions | `action:ReadValue()` (number or Vector2), `IsPressed()`, `WasPressedThisFrame()`, `WasReleasedThisFrame()`, `WasPerformedThisFrame()`, `IsInProgress()`, `triggered`, `phase` ("Disabled", "Waiting", "Started", "Performed", "Canceled"), `activeControl` (path), `enabled`, `Enable()` / `Disable()`, `name`, `type`, `bindings` (`path`, `effectivePath`, `overridePath`, `groups`, `isComposite`, `isPartOfComposite`), `actionMap` |
| Callbacks | `action.started` / `performed` / `canceled`: `AddListener(f)`, `RemoveListener(f)`, `RemoveAllListeners()`; `f(ctx)` with `ctx.action`, `ctx.phase`, `ctx.started` / `performed` / `canceled`, `ctx:ReadValue()`, `ctx:ReadValueAsButton()`, `ctx.time`, `ctx.startTime`, `ctx.duration`, `ctx.interaction`, `ctx.control` |
| Player Input | `GetComponent("PlayerInput")`: `actions`, `currentActionMap` (also settable by name), `SwitchCurrentActionMap(name)`, `currentControlScheme`, `SwitchCurrentControlScheme(name)`, `playerIndex`, `devices`, `inputIsActive`, `ActivateInput()`, `DeactivateInput()`, `GetDevice("Gamepad")`, `notificationBehavior`, events `onActionTriggered`, `onControlsChanged`, `onDeviceLost`, `onDeviceRegained`; `PlayerInput.all`, `PlayerInput.GetPlayerByIndex(i)` |
| Rebinding | `action:GetBindingDisplayString(index?)` ("W", "W/A/S/D", "Shift+B"), `ApplyBindingOverride(index, path)` / `(path)`, `RemoveBindingOverride(index)`, `RemoveAllBindingOverrides()`; `actions:SaveBindingOverridesAsJson()`, `LoadBindingOverridesFromJson(json)`, `RemoveAllBindingOverrides()`; `action:PerformInteractiveRebinding(index?)` then `:WithControlsExcluding(path)`, `:WithCancelingThrough(path)` (default `<Keyboard>/escape`), `:WithTargetBinding(i)`, `:OnMatchWaitForAnother(s)`, `:OnComplete(f)`, `:OnCancel(f)`, `:Start()`, `:Cancel()`, `:Dispose()`, `op.completed`, `op.canceled`, `op.selectedControl` |
| Devices | `Gamepad.current` (last used), `Gamepad.all`: `name`, `leftStick`, `rightStick`, `dpad`, `leftTrigger`, `rightTrigger`, `buttonSouth.isPressed` / `wasPressedThisFrame` / `wasReleasedThisFrame` (and the other buttons), `SetMotorSpeeds(low, high)`, `ResetHaptics()`, `PauseHaptics()`; `Keyboard.current.spaceKey.isPressed`, `Mouse.current.delta`, `Mouse.current.leftButton.wasPressedThisFrame`; `Touchscreen.current.primaryTouch`, `.touches`; legacy `Input.touchCount`, `Input.GetTouch(i)` (`fingerId`, `position`, `deltaPosition`, `phase`), `Input.touches`, `Input.touchSupported` |

Rebinding listens for the next key, mouse button, gamepad button, trigger or stick direction (a whole stick for
Vector 2 bindings), skips `<Mouse>/position` and `<Mouse>/delta`, disables the action meanwhile and turns it back
on once the chosen control is released. Overrides are kept by binding id; save them with PlayerPrefs:

```lua
local saved = PlayerPrefs.GetString("rebinds", "")
if saved ~= "" then actions:LoadBindingOverridesFromJson(saved) end
jump:PerformInteractiveRebinding(0)
    :WithCancelingThrough("<Keyboard>/escape")
    :OnComplete(function(op)
        PlayerPrefs.SetString("rebinds", actions:SaveBindingOverridesAsJson())
        PlayerPrefs.Save()
        op:Dispose()
    end)
    :Start()
```

An action enabled while its button is already held (say the map comes back on with the key still down) doesn't
count that as a press. Gamepad rumble uses force feedback where the platform has it (Windows and Linux through
`gilrs`; ignored elsewhere) and stops when Play stops. Touches come from the window's touch events.

**Active Input Handling** (Edit ▸ Project Settings ▸ Input Manager): Input Manager (actions never read devices),
Input System (the `Input.GetAxis` / `GetButton*` / `GetKey*` / `GetMouseButton*` functions raise an error) or Both
(the default).

Sample: `Scenes/InputSystemDemo.scene` — the First Person Player with a Player Input on `Input/Player.inputactions`
(`FirstPersonController.lua` reads Move, Look, Jump, Sprint and Crouch when a Player Input is on its object),
`PlayerActions.lua` (Send Messages: holding E / X rumbles the pad) and `RebindMenu.lua` (Tab / Start: rebinding
saved to PlayerPrefs).

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
Create the prefab by dragging an object from the Scene Tree into Asset Files.

### Loading addressable assets

```lua
Assets.Run(function()
    local h = Assets.InstantiateAsync("Enemies/Ghost", self.transform):Await()
    Debug.Log("spawned", h.result.name)
    Assets.Release(h)   -- destroys the instance, unloads unused bundles
end)
Assets.LoadAsync("props"):OnCompleted(function(h) Debug.Log(#h.results, "props") end)
```
Tick **Load by Address** in the Properties panel or use the Asset Groups window; see `docs/asset_groups.md`.

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

#### Layers, blend trees and root motion

```lua
local Walker = { damp = 0.15 }
function Walker:Start()
    self._anim = self.gameObject:GetComponent("Animator")
    self._upper = self._anim:GetLayerIndex("Upper Body")
end
function Walker:Update()
    local a, dt = self._anim, Time.deltaTime
    -- Damped: the 2D blend tree eases between walk directions instead of snapping.
    a:SetFloat("MoveX", Input.GetAxis("Horizontal"), self.damp, dt)
    a:SetFloat("MoveZ", Input.GetAxis("Vertical"), self.damp, dt)
    a:SetLayerWeight(self._upper, Input.GetKey("q") and 1 or 0)
    local info = a:GetCurrentAnimatorStateInfo(0)
    if info:IsTag("Emote") then Debug.Log("emoting: " .. info.fullPath) end
end
return Walker
```
Layer 0 is the Base Layer (its weight is always 1); later layers override or add to it, limited to
the bones of their Avatar Mask. With **Apply Root Motion** on the Animator, the clips' root movement
moves the object (through its Character Controller if it has one) instead of the clips setting its
position. A script that defines `OnAnimatorMove()` handles the motion itself:

```lua
function Walker:OnAnimatorMove()
    local a = self._anim
    -- Keep the clip's travel but stay on the ground, and turn with the clip.
    local d = a.deltaPosition
    self.transform.position = self.transform.position + Vector3.new(d.x, 0, d.z)
    self.transform:Rotate(0, a.deltaRotation.y, 0)
end
```
`MatchTarget(position, rotation, "Root", { positionXYZWeight = Vector3.new(0, 1, 1), rotationWeight = 0 },
0.2, 0.6)` moves the object so it reaches `position` when the current state is at normalised time
0.6 (from 0.2). Sample: `Assets/Scripts/CharacterControl.lua` in `Scenes/CharacterAnimatorDemo.scene`
(WASD and Shift walk and run through a 2D blend tree with root motion, Q fades in the masked Upper
Body layer, 1 / 2 enter the Emotes sub-state machine; the second character plays the wave retargeted
through humanoid Avatars).

### UI buttons

Create a button with **Object ▸ UI ▸ Button** (a Canvas is added if the scene has none). In its Button
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

Widgets report changes in two ways:

- **In the Properties panel:** On Value Changed / On End Edit lists (Toggle, Slider, Scrollbar, Input Field, Dropdown)
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

Keyboard and gamepad navigation moves the selection between selectables (each one's Navigation: Automatic,
Horizontal, Vertical, Explicit or None). The Event System reads Navigate / Submit / Cancel from the "UI" map of
its Actions Asset (the default UI actions when none is set: arrows, Return, Esc, the gamepad's stick, d-pad,
South and East); with Active Input Handling set to the Input Manager it reads the arrow keys, Return / Space /
Esc and the first joystick. Cancel closes an open dropdown, or calls `OnCancel()` on the selected object's
scripts. Sample: `Assets/Scenes/UIShowcase.scene` with `Assets/Scripts/UIShowcase.lua` (a gamepad menu, a
radial health ring driven by `fillAmount`, a World Space sign and a Screen Space - Camera HUD).

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
| `moon` | Moon on/off: the disc, the stars and the moonlight at night (boolean). |
| `moonMode` | `"FollowSun"` (on the sun's path, `moonPhase` × 360° east of it), `"Manual"` or `"DateTime"`. |
| `moonPhase` | 0..1: 0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter (FollowSun, Manual). |
| `moonAzimuth`, `moonAltitude` | Manual position in degrees (azimuth from north towards east). |
| `moonDate`, `moonTime`, `moonUtcOffset` | DateTime: `"YYYY-MM-DD"`, local hours, hours east of UTC. |
| `moonLatitude`, `moonLongitude` | Observer position in degrees (north / east positive). |
| `moonSize`, `moonBrightness`, `moonEarthshine` | Disc size (× real), brightness and dark-side earthshine. |
| `moonLightIntensity`, `moonLightColor` | Moonlight for a full moon high up, and its tint. |
| `stars` | Star field brightness at night (0 = none). |
| `moonTexture` | Project path of the moon image, `nil` = built-in. |
| `moonIllumination` | Lit fraction of the disc, 0..1 (read-only). |
| `moonDirection` | `Vector3` towards the moon, `nil` when it is off (read-only). |
| `nightSky`, `nightSkyDarkness` | Night sky on/off; darkening of the sky colour (0..1, the stars stay). |
| `stars`, `starDensity`, `starMagnitudeSpread`, `starGlow` | Star brightness, count (×), spread of magnitudes (0..1), glow of the bright ones. |
| `starColorVariation`, `starTwinkle`, `starTwinkleSpeed`, `starFade` | Colour temperature spread, twinkle and its speed, fading in a bright sky. |
| `skyTime` | Sidereal time in hours (turns the stars and the Milky Way; not in DateTime mode). |
| `milkyWay`, `milkyWayCore`, `milkyWayDust`, `milkyWayWidth`, `milkyWayTint` | Milky Way visibility, core, dust lanes, width, tint (`Color`). |
| `lightPollution`, `lightPollutionColor` | Town glow on the horizon and its colour. |
| `aurora`, `auroraColor`, `auroraTopColor` | Aurora visibility and its lower / top colours. |
| `auroraHeight`, `auroraThickness`, `auroraCurtains`, `auroraWaviness`, `auroraRays` | Curtain height and thickness (km), count (1–6), folds, ray detail. |
| `auroraAzimuth`, `auroraDistance`, `auroraSpeed`, `auroraPulse`, `auroraAmbient` | Direction (°), distance (km), animation speed, pulsing, light on the scene. |
| `meteorRate`, `meteorBrightness` | Shooting stars per minute and their brightness. |

```lua
local Dusk = { speed = 0.02 }
function Dusk:Start() RenderSettings.fog = true end
function Dusk:Update()
    RenderSettings.fogDensity = Mathf.Min(RenderSettings.fogDensity + self.speed * Time.deltaTime, 0.1)
    RenderSettings.fogColor = Color.new(0.8, 0.55, 0.4)
end
return Dusk
```
The fog mode and the skybox are not scriptable yet. `Assets/Scripts/MoonCycle.lua` (MoonDemo) runs `moonPhase` round.

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

Add **Archetype ▸ Physics ▸ Character Controller** (it is its own capsule collider; remove other colliders or
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

### First Person Player

**Object ▸ First Person Player** creates a ready-to-play first-person rig (the sample project also has it
as `Assets/Prefabs/First Person Player.prefab`, played in `Scenes/FirstPersonDemo.scene`, a test course with
ramps, stairs, platforms, a narrow bridge and a low tunnel):

```
Player              Character Controller (1.8 m, radius 0.35) + FirstPersonController.lua
└ HeadBobVertical   HeadBobVertical.lua      (eye height 1.7 m)
  └ HeadBobHorizontal  HeadBobHorizontal.lua
    └ Camera        Camera + Audio Listener + HeadBobRotation.lua
```

The scripts are added to `Assets/Scripts` when the project has none (never overwritten), so every field below
can be tuned in the Properties panel. WASD moves (Horizontal / Vertical), Shift runs (Run), Space jumps (Jump), Ctrl
crouches (Crouch), the mouse looks (Mouse X / Mouse Y) and Escape gives the cursor back (a click takes it
again). Add a **Player Input** to the Player and the controller reads its Move, Look, Jump, Sprint and Crouch
actions instead (the mouse Look binding scaled by 0.1 like Mouse X / Mouse Y; the right stick turns at
`stickLookSpeed` degrees per second); while that map is off it stands still and leaves the cursor alone — see
`Scenes/InputSystemDemo.scene` and `Input/Player.inputactions`.

**FirstPersonController** owns the pose: mouse look turns the Player (yaw) and pitches the Camera; nothing else
rotates them. Locomotion is a small state machine (Idle, Walk, Run, Jump, Fall, Land, Crouch) whose planar
velocity is SmoothDamp'd towards the wanted speed (`walkSpeed` 4 m/s, `runSpeed` 7, `crouchSpeed` 2;
`accelerationTime` / `decelerationTime`, `airControl` scales it in the air), with `coyoteTime` (a jump still
works 0.12 s after a ledge), `jumpBuffer` (a press 0.15 s before landing jumps on landing), `jumpHeight`,
`gravity` and `terminalVelocity`, a sprint FOV kick (`sprintFov`, `fovSmoothTime`), crouching that shrinks the
capsule (`crouchHeight`, stands up only with headroom) and slope / step handling from the Character Controller.
It publishes a **locomotion signal** (`GetComponent("FirstPersonController").signal`) every Update: `state`,
`speed` (measured planar m/s), `speed01`, `grounded`, `airTime`, `phase` (stride phase in radians, advanced by
the distance walked, one stride = two footsteps of `walkStepLength` 1.6 m / `runStepLength` 2.2 m, so the bob follows the footsteps and stops with them),
`phaseRate`, `landing` (0..1 impulse on the landing frame, from the fall speed between `landingMinSpeed` and
`landingMaxSpeed`), `strafe`, `forward`, `turnRate` (deg/s), `crouch` (0..1) and `pitch`.

The three bob scripts find that signal on a parent and only **add offsets to the base pose** they stored at
Start (position for the two bob objects, pitch from the controller for the camera), each LateUpdate:

- **HeadBobVertical**: `amplitude(speed) · sin(2·phase)` (two bounces per stride; `walkAmplitude` 3 cm,
  `runAmplitude` 5 cm, blended with the speed through SmoothDamp over `blendTime`, 0 in the air), a
  spring-damper dip on landing (`landingDip`, `landingFrequency`, `landingDamping`), a slow breathing sine
  while idle (`breathingAmplitude`, `breathingRate`) and the crouch drop (`crouchDrop`). Stopping freezes the
  phase and fades the amplitude, so the head settles instead of snapping.
- **HeadBobHorizontal**: `amplitude(speed) · sin(phase)` (one sway per stride, `walkAmplitude` 2 cm,
  `runAmplitude` 3.5 cm) plus a lean into strafes (`strafeLean`).
- **HeadBobRotation** (on the Camera): a stride roll (`walkRoll` 0.8°, `runRoll` 1.6°), a lean into strafes
  (`strafeTilt`), a counter-roll on fast mouse turns (`turnTilt`, `maxTurnTilt`), a pitch bob from the vertical
  bob's velocity (`pitchBob`, `maxPitchBob`) and a landing pitch kick that springs back (`landingKick`), all
  SmoothDamp'd (`smoothTime`) and clamped (`maxRoll`, `maxPitch`).

The maths is in plain functions (`FirstPersonController.Locomotion`, `FirstPersonController.look`,
`HeadBob*:Evaluate(signal, dt)`), so a script of your own can drive the bob from a different mover by
publishing the same signal table. Tests: `Assets/Tests/FirstPersonTests.playtest.lua`.

### Wheel Collider

Add **Archetype ▸ Physics ▸ Wheel Collider** to a child of an object with a Rigidbody (one per wheel). Each
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

Fixed, Hinge and Spring Joints are set up in the Properties panel (Connected Body = another object with a Rigidbody, or
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
`Input.mousePosition` is the pointer in Scene Viewer pixels from the bottom-left (z = 0).

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
The Hub's **2D Platformer** template starts with its own versions of both: the controller also
jumps with W or Up, remembers a jump pressed just before landing, allows one just after leaving a
ledge and makes a short hop when the key is let go early; the camera stops at the level's edges.

## Scripting defines

A Build Profile's **Scripting Defines** (File ▸ Build Profiles) turn blocks of Lua on and off:

```lua
--#if DEMO
local lastLevel = 3
--#elif !STEAM && DEVELOPMENT_BUILD
local lastLevel = 10
--#else
local lastLevel = 30
--#endif
```

A condition is a define, `!NAME`, or several joined with `&&` (binds tighter) and `||`. Lines left
out become blank, so error line numbers stay right. `Application.HasDefine("DEMO")` asks at run
time. Built-in defines: `MISTY_STANDALONE_WIN` / `MISTY_STANDALONE_LINUX` / `MISTY_STANDALONE_OSX`
(the profile's platform), `MISTY_SERVER` (Dedicated Server), `DEVELOPMENT_BUILD` (builds with
Development Build on) and `MISTY_EDITOR` (Play mode, which uses the active profile's defines).

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
| `Input.simulateMouseDelta(dx, dy)` | Mouse movement for the next frame (pixels, up positive: the Mouse X / Mouse Y axes and `Input.mouseDelta` see it) |
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
`VehicleTests.playtest.lua` (VehicleDemo), `FirstPersonTests.playtest.lua` (FirstPersonDemo).

### Save games
Give each object whose state should be saved a **Saveable** component (Archetype ▸ Miscellaneous). It has a
stable **Save ID** (a GUID; copies, duplicates and prefab instances get new ones) and options: Transform,
Active State, Rigidbody Velocity, and extra component data (`"Light"` = the whole component,
`"Light.intensity"` = one field).

| | |
|---|---|
| `SaveSystem.Save(slot)` | writes every Saveable's state, the prefab instances spawned with `Instantiate("….prefab")`, the scene objects destroyed since the scene started and the scripts' `OnSave` tables; returns true/false |
| `SaveSystem.Load(slot)` | after the current frame, reloads the saved scene from its file, then destroys the destroyed objects, recreates spawned prefabs (by GUID), applies the states and calls `OnLoad(data)` (after `Awake`, before `Start`); returns false when the slot can't be read |
| `SaveSystem.Delete(slot)` / `Exists(slot)` | |
| `SaveSystem.ListSlots()` | newest first: `{ slot, time, date, scene, playTime, playTimeText, version, size, thumbnail }` (`thumbnail` = PNG path or nil) |
| `SaveSystem.GetPlayTime()` | seconds played, carried over from the loaded save |
| `SaveSystem.version` | the game's data version written to saves (default 1) |
| `SaveSystem.OnMigrate = function(data, fromVersion) … return data end` | runs when an older save is loaded |
| `SaveSystem.compress` | gzip the save file (default false; loading detects either) |
| `SaveSystem.thumbnails` | save a PNG of the last frame next to the save (default true) |

Scripts on Saveable objects keep their own values:

```lua
function Counter:OnSave(data) data.coins = self.coins; data.spawn = self.transform.position end
function Counter:OnLoad(data) self.coins = data.coins or 0 end
```
`data` holds numbers, strings, booleans, Vector3 and nested tables. Slots are names or numbers. Saves go to
`Library/Saves` in the editor (Window ▸ General ▸ Save Data lists, shows and deletes them) and to the
player's user folder in built games (`%APPDATA%\<Company>\<Product>\saves` on Windows, per Player Settings).
Each slot is `<slot>.save` (JSON), `<slot>.info.json` and an optional `<slot>.png`.
`GetComponent("Saveable").saveId` reads the Save ID.

### Localization
Project Settings ▸ Localization lists the locales (empty = every column of the String Tables), the
default and fallback locale and the **Startup Locale** (Saved Preference — the player's last choice, else
the system language —, System Language, or Default Locale). Texts live in **String Tables** (`.strings`
files anywhere under `Assets/`, edited in Window ▸ General ▸ Localization Tables, CSV import/export).

| | |
|---|---|
| `Localization.locale` | current locale code; set it to switch (also remembered in PlayerPrefs) |
| `Localization.locales` | `{ code, name }` list |
| `Localization.Get(key [, args [, table]])` | the text in the current locale, else the fallback, else the key itself |
| `Localization.GetAsset(key [, table])` | Asset Tables (`"kind": "assets"`): the asset path for the locale |
| `Localization.HasKey(key [, table])`, `Localization.isRightToLeft` | |
| `Localization.OnLocaleChanged:AddListener(f)` | `f(code)` after a switch; scripts may also define `OnLocaleChanged(code)` |
| `GetComponent("LocalizedText")` | `key`, `table`, `args` (table), `enabled`, `text` (read-only), `RefreshString()` |

Smart strings: `{0}`, `{1}` (positional `args`), `{name}` (`args.name`), `{{`/`}}` for braces, and plurals
`{0:plural:one apple|{} apples}` — forms in the language's order (French: 0 and 1 singular; Arabic:
zero|one|two|few|many|other; Russian/Polish: one|few|many; Japanese/Chinese: one form).

```lua
Localization.Get("score", { 12 })                 -- "{0} points"
Localization.Get("greeting", { name = "Ana" })    -- "Hello, {name}!"
self.gameObject:GetComponent("LocalizedText").args = { self.coins }
```
The **Localized Text** component (Archetype ▸ UI) sets the UI Text on its object from a key and arguments
(Properties entries `name=value` fill `{name}`) and follows locale changes. Right-to-left scripts are not
laid out right to left yet, and the built-in UI font has no Arabic glyphs.

Sample: `Scenes/SaveLocDemo.scene` with `SaveLocMenu.lua`, `CoinCounter.lua` and
`Assets/Localization/SaveLocDemo.strings`.

## Networking
Multiplayer works like a familiar netcode package: one **Network Manager** starts a host (server + local
player), a dedicated server or a client; the server **spawns** objects with a **Network Object** on every
client and gives each an **owner**; a **Network Transform** replicates motion; scripts replicate fields with
**Network Variables** and call functions across the network with **RPCs**. Traffic goes over UDP (reliable
ordered, reliable unordered and unreliable channels).

**Setup**: add a Network Manager (Archetype ▸ Netcode) to one object in the scene: Address and Port (where
clients connect, 127.0.0.1:7777 by default), Listen Address, Tick Rate (network updates per second), Player
Prefab (spawned for every client that joins, owned by it), Connection Approval, Max Connections, Timeout,
Scene Management (clients load the server's scene when they join and whenever it changes), LAN Discovery,
and a **Network Simulator** (latency, jitter, packet loss) for testing. Give every networked object (and
the player prefab) a Network Object.

**Starting**: from a script (`Network.StartHost()`, …), from the Network Manager Properties panel's Start Host /
Start Server / Start Client buttons while playing, or with **Window ▸ Multiplayer ▸ Multiplayer Play
Mode**: tick Player 2–4 and press Play; the editor hosts and opens a player window per ticked player that
joins as a client on this machine (the windows load the scene as it was when Play was pressed, saved or
not, and close on Stop). A small overlay in the Scene Viewer shows the round trip time, packets and bandwidth.
Built games: `misty_player --net-host`, `--net-server` or `--net-client 192.168.1.20:7777`.

| | |
|---|---|
| `Network.StartHost()` / `StartServer()` / `StartClient([address [, port]])` | start (address defaults to the Network Manager's); true when it started |
| `Network.Shutdown()` | stop: spawned prefab copies go, scene objects stay |
| `Network.IsServer`, `IsClient`, `IsHost`, `IsListening`, `IsConnectedClient` | |
| `Network.LocalClientId`, `ServerClientId` (0), `Mode` ("Host"/"Server"/"Client"/"Offline") | the host's own player is client 0 |
| `Network.ConnectedClientsIds` | client ids (server: everyone; a client: itself) |
| `Network.LocalPlayer`, `Network.GetPlayerObject(clientId)`, `Network.GetNetworkObject(id)` | GameObjects or nil |
| `Network.Spawn(prefabPath or gameObject [, position [, ownerClientId]])` | server: instantiate (or take) and spawn on every client; returns the GameObject |
| `Network.DisconnectClient(clientId)` | server: kick |
| `Network.connectionData` | text sent with the connect request (a name, a password) |
| `Network.ConnectionApproval = function(clientId, payload) return true end` | server, with Connection Approval on: return `false, "reason"` to refuse |
| `Network.OnClientConnected` / `OnClientDisconnected` | `:AddListener(function(clientId) end)`; on the server for every client, on a client for itself |
| `Network.OnServerStarted` / `OnClientStarted` / `OnServerStopped` / `OnClientStopped` | `:AddListener(f)` |
| `Network.SenderClientId` | inside a server RPC: who called it |
| `Network.FindLanGames()`, `Network.LanGames` | broadcast a search; the list fills with `{ address, port, name, players, maxPlayers }` |

**Scripts on network objects** (on the object or its children) read `self.IsOwner`, `self.IsServer`,
`self.IsClient`, `self.IsHost`, `self.IsLocalPlayer`, `self.IsOwnedByServer`, `self.OwnerClientId`,
`self.NetworkObjectId`, `self.IsSpawned` and `self.NetworkObject`, and get the callbacks
`OnNetworkSpawn()` (Network Variables already hold the server's values), `OnNetworkDespawn()`,
`OnGainedOwnership()`, `OnLostOwnership()` and `OnOwnershipChanged(previous, current)`.

`GetComponent("NetworkObject")`: `NetworkObjectId`, `OwnerClientId`, `IsOwner`, `IsSpawned`,
`IsPlayerObject`, `IsLocalPlayer`, `Spawn([owner])`, `SpawnWithOwnership(id)`, `SpawnAsPlayerObject(id)`,
`Despawn([destroy = true])`, `ChangeOwnership(id)`, `RemoveOwnership()` (server only).
`GetComponent("NetworkTransform")`: `authority`, `CanCommitToTransform`, `interpolate`,
`Teleport(position [, euler])`.

Objects saved in the scene are matched between server and clients by the Network Object's **Global ID**
(copies get a new one); spawned prefab copies are built from their prefab, so spawn prefabs
(`Network.Spawn("Assets/Prefabs/Ball.prefab")`, or `Instantiate(path)` then `:Spawn()`). On the server,
`Destroy` of a spawned object despawns it everywhere. Only the server spawns, despawns and changes owners.

### Network Transform
**Authority** Server (the server moves it, clients follow) or Owner (the owning client moves it; the server
relays it). Remote copies **interpolate** between received states, played *Interpolation Delay* (0.1 s)
in the past so late packets don't show; jumps longer than **Snap Distance** teleport. Only changes larger
than the position / rotation / scale **thresholds** are sent (and each part only when it changed), with a
full state every second. Rigidbodies moved by another peer become kinematic there.

**Predict** (server authority): the owner moves its own copy at once and also sends its input in a server
RPC; the server moves the real object, and its states tell the owner which input frame they include, so
the owner corrects itself by however far its prediction was off (reconciliation) without waiting a round
trip. Move by adding deltas, the same way on both sides:

```lua
function Player:Update()
    if not self.IsOwner then return end
    local delta = Vector3.new(Input.GetAxis("Horizontal"), 0, Input.GetAxis("Vertical")) * (5 * Time.deltaTime)
    self.transform.position = self.transform.position + delta          -- predicted here
    if not self.IsServer then self:MoveServerRpc(delta) end
end
function Player:MoveServerRpc(delta)
    if delta.magnitude < 0.5 then self.transform.position = self.transform.position + delta end
end
```

### Network Variables
Mark fields with `NetworkVariable()` in `_attributes`. The writer changes the field normally; the change is
sent at the next tick (only changed fields go out), and every peer's `onChanged` method gets
`(previous, current)`. Values: numbers, booleans, strings, Vector3, tables of those, and spawned
GameObjects.

```lua
local Player = { score = 0, nickname = "", secret = "" }
Player._attributes = {
    score = NetworkVariable({ onChanged = "OnScoreChanged" }),     -- the server writes, everyone reads
    nickname = NetworkVariable({ write = "Owner" }),               -- the owner writes
    secret = NetworkVariable({ read = "Owner" }),                  -- only the owner (and server) read
}
function Player:OnScoreChanged(old, new) Debug.Log("score", new) end
```
A peer that may not write a variable gets the change undone with a warning.

### RPCs
Functions named `…ServerRpc` run on the server when a client calls them; `…ClientRpc` run on clients when
the server calls them (on the host too). Call them with a colon as usual; arguments are sent like Network
Variable values. Options in `_attributes`: `ServerRpc({ requireOwnership = false })` (by default only the
owner may call a server RPC), `reliable = false` (send unreliably, e.g. frequent input). A client RPC goes
to every client unless the last argument is an `RpcTarget`: `RpcTarget.Owner`, `NotOwner`, `NotServer`,
`Client(id)`, `Clients({ ids })`.

```lua
function Gun:Update()
    if self.IsOwner and Input.GetMouseButtonDown(0) then self:FireServerRpc(self.transform.forward) end
end
function Gun:FireServerRpc(dir)                     -- on the server; Network.SenderClientId is the caller
    self:ShowShotClientRpc(dir, RpcTarget.NotOwner)
end
function Gun:ShowShotClientRpc(dir) end              -- on the other clients
```

Sample: `Scenes/NetworkDemo.scene` (H host, J join, WASD move; or Multiplayer Play Mode with players
ticked): `Net Player.prefab` with `NetPlayer.lua` (predicted movement, owner-written name, server-written
score and colour), `NetCoin.lua` (server pickups, a client RPC) and `NetHud.lua` (keys and scoreboard).
Not done yet: relay servers and NAT punch-through (players on different networks need a reachable host),
dedicated-server builds without a window, Network Animator / Rigidbody components.

## Editor scripting
Lua files in a folder named `Editor` (anywhere under `Assets`, e.g. `Assets/Editor/` or
`Assets/Tools/Editor/`) are **editor scripts**: they add menu items, tool windows, custom Properties views, Scene
view gizmos and handles, and asset hooks. They run in the editor's own Lua state, never in a build (builds
leave every `Editor` folder out), and reload by themselves when one is saved. The game API is there too,
so `GameObject.Find`, `transform.position`, `Vector3` and `Debug.Log` work on the scene being edited.

Errors go to the Console with file and line (click to open the script). A callback that fails stays off
until its script changes (tool windows and Properties views show a Try again button), and one that runs too long
is stopped: 0.5 s per frame for `OnGUI`, Properties views, gizmos and handles, 5 s for a menu item, a script
loading, an update callback or an asset hook.

Sample: `Assets/Editor/ScatterTool.lua` (Tools ▸ Scatter Tool and Tools ▸ Align to Floor),
`Assets/Editor/SatelliteInspector.lua` and `Assets/Scripts/Satellite.lua` (add the Satellite script to any
object to see its gizmos, its custom Properties view and the centre handle).

### Menu items
```lua
MenuItem("Tools/Scatter Tool", function() Scatter:Open() end)
MenuItem("Tools/Align to Floor", alignSelection, {
    validate = function() return Selection.count > 0 end,  -- greyed out while false
    shortcut = "Ctrl+Alt+G",                                 -- Ctrl / Shift / Alt + a key
    priority = 10,                                           -- order in its menu (lower first)
})
```
A new top-level name ("Tools") becomes a menu between Component and Window; a path under a built-in menu
("GameObject/My Item") goes at the end of that menu; more slashes make submenus. Every item is in the
Ctrl+K search too. Built-in shortcuts win over script ones.

### Tool windows
```lua
local Counter = ToolWindow("Counter")
Counter.count = 0                          -- plain values on the window are its state
function Counter:OnGUI()
    self.count = GUI.IntField("Count", self.count)
    if GUI.Button("Add one") then self.count = self.count + 1 end
end
MenuItem("Tools/Counter", function() Counter:Open() end)
```
A tool window is a dock tab like the built-in panels: it opens by the Properties panel, docks and floats anywhere,
and layouts remember it. Its numbers, booleans, strings, Vector3s and Colors are kept across reloads and
restarts (`Library/ToolWindows.json`). `Window:Open()`, `Window:Close()`, `Window:Repaint()`; `OnEnable()`
runs after each load.

| GUI (inside `OnGUI` / `OnInspectorGUI`) | Returns |
|---|---|
| `GUI.Label(text)`, `GUI.Header(text)`, `GUI.HelpBox(text [, kind])` | (`kind`: "info", "warning", "error") |
| `GUI.Button(text)` | true on the frame it is clicked |
| `GUI.Toggle(label, on)`, `GUI.Slider(label, v, min, max)` | the new value |
| `GUI.IntField(label, v)`, `GUI.FloatField(label, v)`, `GUI.TextField(label, s)` | the new value |
| `GUI.ColorField(label, color)`, `GUI.Vector3Field(label, v)` | the new value |
| `GUI.ObjectField(label, value [, kind])` | a GameObject (`kind` "GameObject", the default) or an asset path of that kind ("prefab", "material", "texture", "model", "audio", "script" or an extension); pick from the list or drop an asset from Asset Files |
| `GUI.Foldout(label, open)` | the new open state (draw the contents yourself while open) |
| `GUI.Horizontal(fn)`, `GUI.Vertical(fn)`, `GUI.Scroll(fn)` | lay out what `fn` draws |
| `GUI.Space([px])`, `GUI.Separator()` | |

### Custom Properties views
```lua
local SatelliteInspector = CustomInspector("Satellite")    -- the script's file name
function SatelliteInspector:OnInspectorGUI()
    DrawDefaultInspector()                              -- leave out to replace the field list
    if GUI.Button("Centre on origin") then
        self.target.center = Vector3.zero               -- an undoable edit, like typing in the field
    end
end
function SatelliteInspector:OnSceneGUI()                  -- while the object is the active selection
    self.target.center = Handles.Position(self.target.center)
    Handles.Label(self.target.center, "Orbit centre")
end
```
`self.target` reads and writes the script's public fields (the saved value, else the default) and has
`gameObject` and `transform`. One custom Properties view instance lives per inspected script, so values kept on `self`
last while it stays selected.

### Gizmos and handles
Game scripts can define `OnDrawGizmos()` (every object) and `OnDrawGizmosSelected()` (selected objects).
They run in the Editor Scene while editing (with the Gizmos toggle on), never in Play mode or builds; `self`
has the script's field values, `gameObject` and `transform`.

| | |
|---|---|
| `Gizmos.color = Color.new(r, g, b [, a])` | colour of what follows (white at the start of each call) |
| `Gizmos.Line(a, b)`, `Gizmos.Ray(from, direction)` | lines |
| `Gizmos.WireSphere(center, radius)`, `Gizmos.WireCube(center, size)` | wire shapes |
| `Gizmos.Icon(position [, glyph])`, `Gizmos.Label(position, text)` | a glyph / a text at a point |
| `Handles.Position(p)` (in `OnSceneGUI`) | a three-axis move handle; returns where it was dragged |
| `Handles.Label(p, text)` | a text at a point |

### Editor API
| | |
|---|---|
| `Selection.active`, `Selection.objects`, `Selection.asset` | the active GameObject, all selected ones, the asset selected in Asset Files (a path); all assignable |
| `Selection.count`, `Selection.Clear()`, `Selection.Contains(go)` | |
| `Undo.Record(obj, "Name")` | call before changing objects: everything until the call returns is one undo step (it also marks the scene changed) |
| `Undo.BeginGroup("Name")` … `Undo.EndGroup()` | one step over several calls (an update callback working over frames) |
| `Undo.Undo()`, `Undo.Redo()` | |
| `EditorScene.Find(name)`, `EditorScene.FindAll(query)`, `EditorScene.Roots()` | `query`: "t:Light" (a component or script name) or part of an object name |
| `EditorScene.Create(name [, parent])`, `EditorScene.InstantiatePrefab(path [, position [, parent]])` | new objects (prefab instances stay linked to the prefab) |
| `EditorScene.AddComponent(go, name)`, `EditorScene.HasComponent(go, name)` | a component name or a `.lua` script path |
| `EditorScene.SetParent(go, parent)`, `EditorScene.Destroy(go)` | |
| `AssetDB.Find(query [, folder])` | asset paths: "t:prefab" / "t:texture" / … and words of the file name |
| `AssetDB.Exists(path)`, `AssetDB.ReadText(path)`, `AssetDB.Refresh()` | |
| `AssetDB.CreateAsset(path, text)`, `AssetDB.CreateFolder(path)`, `AssetDB.CreateMaterial(path [, color])` | inside `Assets/` only |
| `EditorPrefs.GetInt` / `SetInt`, `GetFloat` / `SetFloat`, `GetString` / `SetString`, `GetBool` / `SetBool`, `HasKey`, `DeleteKey` | per user, shared by all projects |
| `Editor.DisplayDialog(title, message [, ok [, cancel]])` | a message box; true for OK |
| `Editor.ShowProgress(title, info, fraction)`, `Editor.ClearProgress()` | a progress bar at the bottom of the window |
| `Editor.AddUpdate(fn)`, `Editor.RemoveUpdate(fn)` | `fn()` every editor frame (return `false` to stop) |
| `Editor.IsPlaying()`, `Editor.ProjectPath()`, `Editor.Repaint()` | |

The progress bar draws between frames, so a long job splits its work over an update callback:

```lua
local left = 500
Undo.BeginGroup("Place Trees")
Editor.AddUpdate(function()
    for _ = 1, 25 do placeOneTree(); left = left - 1 end
    Editor.ShowProgress("Placing trees", left .. " left", 1 - left / 500)
    if left <= 0 then
        Editor.ClearProgress()
        Undo.EndGroup()
        return false
    end
end)
```

### Asset hooks
```lua
local Hooks = AssetProcessor()
function Hooks:OnPostprocessAsset(path)
    if path:match("%.png$") then Debug.Log("New texture: " .. path) end
end
```
`OnPostprocessAsset(path)` runs for each asset that appears in the project: files copied into `Assets`
(noticed within a couple of seconds), assets the editor creates, and assets made with `AssetDB`. Changes to
an existing file do not call it.
