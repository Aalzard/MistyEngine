# Round 18C — Input System (action-based input)

## Progress

- **Input Actions asset** (`misty_engine::input::actions::asset`, `.inputactions`, JSON): action maps, actions
  (Value / Button / Pass Through, control type Any / Button / Axis / Vector 2, initial state check), bindings
  (control path, groups, processors, interactions, stable ids) and composites (parts with their own paths,
  groups and processors), control schemes (devices, optional or required). `InputActionAsset::starter`
  (Player: Move, Look, Attack, Jump, Sprint, Crouch, Interact; UI: Navigate, Submit, Cancel, Point, Click;
  Keyboard&Mouse, Gamepad, Touch) is what Assets ▸ Create ▸ Input Actions writes.
- **Control paths** (`path.rs`): `<Keyboard>/…` (letters, digits, F1–F24, named keys, `shift` / `ctrl` /
  `alt`, `anyKey`), `<Mouse>/leftButton|rightButton|middleButton|delta|scroll|position[/x|/y]`,
  `<Gamepad>/buttonSouth…start|leftStickPress|leftTrigger|leftStick[/x|/y|/up…]|dpad[/up…]` (Xbox and
  PlayStation letter aliases), `<Touchscreen>/primaryTouch|touchN/position|delta|press`,
  `<Pointer>/position|delta|press`; `{usage}` qualifiers accepted. Display names ("W [Keyboard]"), the
  editor's control lists, key-name ↔ path mapping, reading values through a device filter (paired devices).
- **Composites** (`composites.rs`): 2DVector (DigitalNormalized / Digital / Analog), 1DAxis (whichSideWins,
  min/max), ButtonWithOneModifier, ButtonWithTwoModifiers, OneModifier.
- **Processors** (`processors.rs`): invert, invertVector2, normalize (one- and two-sided), normalizeVector2,
  scale, scaleVector2, clamp, axisDeadzone, stickDeadzone.
- **Interactions** (`interactions.rs`): press (PressOnly / ReleaseOnly / PressAndRelease), hold, tap, slowTap,
  multiTap, all with pressPoint and release hysteresis; stepped every frame against a clock so timeouts fire
  without input changes. Several interactions on one binding hand over (the first to start drives, another
  that started takes over on cancel; the rest reset when one finishes).
- **Runtime** (`state.rs`, `system.rs`): an `ActionSet` per asset copy (enabled maps / actions, binding mask,
  device filter, overrides, per-action phase, value, pressed / released / performed this frame, active control,
  disambiguation by actuation, canceled on disable, priming from the previous frame so a held button doesn't
  count as a press when its map comes back on). `InputSystem` in the scripting world: shared copies
  (`InputActions.Load`) and instances, Player Inputs (start / stop with their objects, Hierarchy order,
  default map), control schemes with auto-switch for a single player, device pairing for local multiplayer
  (keyboard & mouse to the first, one gamepad each after), device lost / regained, interactive rebinding
  (candidates, excluded and cancel controls, wait for another match, action re-enabled after the control is
  released), gamepad rumble requests, the Active Input Handling setting. Updated every frame before Start /
  Update; events reach scripts after Start.
- **Player Input component** (Component ▸ Input): actions asset, default map, default scheme, auto-switch,
  behaviour (Send Messages, Broadcast Messages, Invoke Events with per-action calls stored like On Click lists,
  remapped in scene files and prefabs).
- **Lua API** (`scripting/input_actions_api.rs`): `InputActions.Load / Instantiate`, maps and actions
  (`ReadValue`, `IsPressed`, `WasPressedThisFrame`, `WasReleasedThisFrame`, `WasPerformedThisFrame`,
  `IsInProgress`, `triggered`, `phase`, `activeControl`, `bindings`, `Enable` / `Disable`, `started` /
  `performed` / `canceled` listeners with a callback context), overrides (`ApplyBindingOverride`,
  `RemoveBindingOverride`, `RemoveAllBindingOverrides`, `SaveBindingOverridesAsJson`,
  `LoadBindingOverridesFromJson`, `GetBindingDisplayString`), `PerformInteractiveRebinding` with its builder,
  `GetComponent("PlayerInput")` (`SwitchCurrentActionMap`, `SwitchCurrentControlScheme`, `ActivateInput`,
  `DeactivateInput`, `GetDevice`, events), `PlayerInput.all`, `Gamepad.current / all` (state, rumble),
  `Keyboard.current`, `Mouse.current`, `Touchscreen.current`, `Input.touchCount / GetTouch / touches /
  touchSupported`. With Active Input Handling = Input System the Input Manager functions raise an error.
- **Devices**: touches (`input::touch`, from egui touch events in the Game view and winit touch events in the
  player); rumble through gilrs force feedback (Windows WGI, Linux; a no-op elsewhere), stopped on Stop.
- **Editor**: Input Actions window (Window ▸ Input ▸ Input Actions, double-click an asset): Action Maps |
  Actions with their bindings | Properties, add / rename / duplicate / delete, add binding and the composite
  kinds, a path field with a device ▸ control picker and **Listen** (next key, modifier or gamepad input; Esc
  cancels), Use in Control Scheme, interaction and processor lists with parameter fields, composite type and
  mode, a control scheme filter with Add / Edit / Delete Control Scheme, Save Asset and Auto-Save. Inspector:
  Player Input (asset, schemes, maps, behaviour help, Events lists, a Debug section while playing) and the
  Input Actions asset summary with Edit Asset. Project window icon and `t:inputactionasset`, Assets ▸ Create ▸
  Input Actions, Active Input Handling in Project Settings ▸ Input Manager. `MISTY_INPUT_ACTIONS` /
  `MISTY_INPUT_SELECT` for screenshots.
- **Sample**: `Scenes/InputSystemDemo.scene` (`sample_input_system.rs`, `input_system_demo_matches_its_generator`,
  `input_system_demo_plays_with_actions`) with `Input/Player.inputactions`, `PlayerActions.lua` and
  `RebindMenu.lua`; `FirstPersonController.lua` reads a Player Input's Move / Look / Jump / Sprint / Crouch when
  there is one and the Input Manager axes otherwise (FirstPersonDemo, MeadowDemo and the template unchanged).
- **Tests**: paths, device reads, composites (unit and through actions), processors, interactions on a
  simulated clock, hold + tap hand-over, action types, priming, binding mask, the starter asset, rebinding
  (keys, composite parts, sticks, cancel, excluded) and overrides saved to PlayerPrefs and loaded back, single
  player scheme switching, local multiplayer pairing and device loss, rumble, Input Manager only; Lua: Player
  Input messages with listeners and polling, Invoke Events with a hold and map switching, interactive
  rebinding persisted across runs, Active Input Handling, touches / gamepad / keyboard / rumble; kittest for
  the window (columns, Listen, add map / composite / processor with Auto-Save, scheme filter, Create menu,
  Player Input Inspector); the demo generator and play test.

## Decisions

1. **Bindings live under their action** in the asset (Unity keeps a flat binding list per map with an action
   name); composites hold their parts. Scripts still number bindings Unity's way (composite, then its parts).
2. **Ids** are kept per binding and part for overrides; files without ids get stable ones derived from names
   and positions, so hand-written assets keep their overrides across loads.
3. **Bindings without groups** are used by every control scheme (friendlier for hand-written assets).
4. **Polling** (`IsPressed`, `WasPressedThisFrame`) uses the press point 0.5 with a 0.75 release ratio,
   independent of interactions.
5. **Player Input's first player uses the shared asset copy** (`InputActions.Load` returns the same one);
   further players get their own copies.
6. **The rebound action comes back once its control is released**, so the press that chose the binding isn't
   also a press of the action.
7. **Mouse scroll in notches** (the Input Manager's unit), not raw wheel pixels.
8. **Active Input Handling lives in the Input Manager page** (the reference editor keeps it in Player settings),
   left out of the settings file at its default so existing projects re-save unchanged.

## Not done

- No PlayerInputManager (joining players by pressing a button, split screen).
- The UI map doesn't drive the UI event system (no UI input module); UI still reads the mouse directly.
- No on-screen controls (virtual sticks / buttons) and no touch gestures beyond the primary / indexed touches.
- No Vector 3 composite, no `Any` value type conversions beyond number ↔ Vector 2, no per-device usages
  (`{LeftHand}` is parsed and ignored).
- No generated script wrapper for assets, no undo inside the Input Actions window (Save Asset / Auto-Save
  write the file; Ctrl+Z does not step back edits there).
- Rumble only where gilrs has force feedback; untested on real pads in this round.
