# Round 11 — vehicles: Wheel Collider and a drivable car

## Decisions

1. **Own raycast wheel, not rapier's `DynamicRayCastVehicleController`.** The reference engine's Wheel
   Collider is a component per wheel on child objects, with its own friction curves (extremum/asymptote
   slip and value, stiffness), sprung-mass suspension and motor/brake torque per wheel. rapier's controller
   owns all wheels of one chassis, has a single friction slip value and no friction curves, so mapping the
   component onto it would lose most of the Inspector. `physics/vehicles.rs` casts one ray per wheel and
   applies impulses to the parent body before each fixed step (like Spring Joints and Floaters).
2. **Suspension carries its share of the body's weight plus spring and damper.** Load = body mass / wheel
   count × gravity along the suspension + spring × (rest − extension) + damper × compression speed. So a car
   rests at the spring's Target Position whatever the stiffness (the reference engine's sprung-mass
   behaviour), and the rest-compression test is exact. Target Position: 0 = fully compressed, 1 = fully
   extended (rest extension = target × Suspension Distance). The ground pushes back along its normal, so a
   slightly pitched car doesn't creep on flat ground.
3. **Tyre friction as clamped sequential impulses.** Per grounded wheel, the impulse that would stop the slip
   (forward: rim speed vs ground speed through the wheel's inertia; sideways: speed along the axle) is
   applied in full while it fits under the curve's extremum × load (grip, no low-speed jitter), otherwise the
   tyre slides with the curve's value at the slip that results. Four passes over all wheels (accumulated,
   clamped totals) so wheels sharing a body settle together rather than in Hierarchy order — one pass gave a
   visible yaw drift under throttle. A friction circle (largest curve peak × load) shares grip between the
   two directions, so a handbraked rear axle slides out.
4. **Slip values.** Forward slip = (ground speed − rim speed) / max(|ground|, |rim|, 1 m/s) (negative while
   accelerating, positive while braking, as in the reference engine); sideways slip = atan(sideways speed /
   max(|forward speed|, 1 m/s)). While gripping, the reported slip is where the curve's rising part gives the
   force used (so scripts can still detect skids).
5. **Brakes lock wheels.** Brake torque that can hold the wheel against the tyre's impulse locks it (the tyre
   then grips or slides with the wheel still); weaker brake torque only slows the free wheel after the
   friction pass. Applying the brake only to the free wheel before friction let the tyre spin it up again
   each step and the car barely slowed.
6. **rapier's substep gravity.** rapier integrates gravity over 4 substeps, so a resting body's velocity after
   a step is −g·dt·3/8, not 0. The damper uses the step's mean velocity and friction the velocity the coming
   step would have on average (`mean_velocity` / `step_velocity`), otherwise wheels settle 1 cm low and cars
   creep on slopes.
7. **Runtime state on the component, not saved.** `WheelCollider.state` (`#[serde(skip)]`: rpm, grounded,
   extension, spin, hit) is written by `Physics::sync_out` each frame, so Lua (`rpm`, `isGrounded`,
   `GetWorldPose`, `GetGroundHit`), the Inspector's Live rows and the gizmos read it without reaching into
   physics. Settings (motor/brake/steer) are read from the component each step, so script and Inspector edits
   during Play apply at once.
8. **Wheel model built in.** `wheel_model` (object reference, remapped like joint Connected Bodies) is moved to
   the wheel pose each frame — the built-in alternative to the reference engine's GetWorldPose + user script;
   `GetWorldPose` is still there for custom setups. No Quaternion in Lua, so it returns Euler angles.
9. **Scale ignored, axes fixed.** Like the reference engine the wheel ignores its object's scale; it rolls
   around local X and drives along local Z, steering around local Y (positive = right).
10. **VehicleDemo** is generated in code (`sample_vehicles`), rear-wheel drive with a steering limit that
    shrinks with speed, engine braking when coasting, R to reset; a separate `CarFollowCamera.lua`. Wheel
    models are an empty pose object with tyre, rim and a spoke bar as children, so the spoke shows the roll.

## Progress

- Wheel Collider component (Add Component ▸ Physics), saved and remapped; Inspector with Suspension Spring /
  Forward Friction / Sideways Friction / Drive groups and Live values while playing; gizmos (suspension
  line, tyre circle with a spoke, contact point and normal while playing) for the selected wheel.
- Lua `WheelCollider`: motorTorque, brakeTorque, steerAngle, enabled, rpm, isGrounded, radius,
  suspensionDistance, GetWorldPose, GetGroundHit; code-editor completion; docs/scripting.md section.
- Samples: `Scenes/VehicleDemo.scene`, `Scripts/CarController.lua`, `Scripts/CarFollowCamera.lua`, new
  materials (Car Paint, Tyre Rubber, Wheel Rim, Car Glass, Headlight, Tail Light, Track Checker, Traffic Cone).
- Tests: rest compression and load share, full extension in the air, motor drives forward and reverses,
  rolling without slip, steering turns right and stays upright, brakes stop the car, coasting keeps rolling,
  wheel models follow, missing Rigidbody warns, save/load; Lua driving; Inspector and gizmo tests; the demo's
  generator match and a scripted drive (settle, W, W+D, S, handbrake, camera follows).

Not done: anti-roll bars, wheel-to-wheel differential/engine model (torque is per wheel, as in the reference
engine), sweep (shape-cast) suspension for kerbs; the ray is a single line.
