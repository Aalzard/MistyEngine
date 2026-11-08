-- FirstPersonController: WASD walk, Shift run, Space jump, Ctrl crouch, mouse look.
-- Needs a Character Controller. The Camera is found among the children (the First Person
-- Player prefab is Player > HeadBobVertical > HeadBobHorizontal > Camera): the controller turns
-- the Player (yaw) and pitches the Camera; the head bob scripts only add offsets on top.
--
-- Locomotion is a small state machine (Idle / Walk / Run / Jump / Fall / Land / Crouch) with
-- SmoothDamp acceleration, coyote time, jump buffering, gravity with a terminal velocity and
-- reduced air control. It publishes `self.signal` for the head bob scripts
-- (GetComponent("FirstPersonController").signal): see Locomotion:publish below.
--
-- Input: with a Player Input on the same object, the Move, Look, Jump, Sprint and Crouch
-- actions of its Input Actions asset drive it (Scenes/InputSystemDemo, Input/Player.inputactions);
-- while that map is disabled (a menu switched to another map) the player stands still and
-- leaves the cursor alone. Without one, it reads the Input Manager axes (Horizontal, Vertical,
-- Mouse X, Mouse Y, Run, Crouch, Jump) with key fallbacks.
local FirstPersonController = {
    walkSpeed = 4,
    runSpeed = 7,
    crouchSpeed = 2,
    accelerationTime = 0.12, -- SmoothDamp time towards the target speed (ground)
    decelerationTime = 0.08, -- ... and back to a stop
    airControl = 0.3, -- share of the ground acceleration available in the air
    jumpHeight = 1.2, -- metres
    gravity = 20, -- m/s², downwards
    terminalVelocity = 40, -- m/s
    coyoteTime = 0.12, -- seconds after leaving a ledge a jump still works
    jumpBuffer = 0.15, -- seconds before landing a jump press is remembered
    landDuration = 0.15, -- the Land state after a hard landing
    landingMinSpeed = 4, -- fall speed (m/s) where the landing impulse starts
    landingMaxSpeed = 12, -- fall speed giving a full (1.0) landing impulse
    mouseSensitivity = 2, -- degrees per Mouse X / Mouse Y unit
    stickLookSpeed = 150, -- degrees per second at full right stick (Input System Look action)
    mouseSmoothing = 0, -- seconds of look smoothing (0 = raw)
    pitchMin = -85,
    pitchMax = 85,
    sprintFov = 8, -- degrees added to the camera's field of view while running
    fovSmoothTime = 0.2,
    crouchHeight = 1.2, -- capsule height while crouched
    crouchSmoothTime = 0.15,
    lockCursor = true, -- lock and hide the cursor in Play mode (Esc releases, click locks)
    walkStepLength = 1.6, -- metres per footstep: the head bob frequency follows the steps
    runStepLength = 2.2,
    pushPower = 2, -- rigidbodies walked into get this velocity
}

-- ---- helpers ----------------------------------------------------------------------------------

local function clamp(v, lo, hi)
    if v < lo then return lo end
    if v > hi then return hi end
    return v
end

local function clamp01(v) return clamp(v, 0, 1) end

-- Critically damped spring towards `target` (the familiar SmoothDamp): never overshoots,
-- frame-rate independent. Returns the new value and velocity.
local function smoothDamp(current, target, velocity, smoothTime, dt)
    smoothTime = math.max(0.0001, smoothTime)
    local omega = 2 / smoothTime
    local x = omega * dt
    local exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x)
    local change = current - target
    local temp = (velocity + omega * change) * dt
    velocity = (velocity - omega * temp) * exp
    local out = target + (change + temp) * exp
    -- Don't pass the target.
    if (target - current > 0) == (out > target) then
        out = target
        velocity = (out - target) / dt
    end
    return out, velocity
end

-- Input Manager buttons and axes with a key fallback for projects without the entry.
local function button(name, key)
    local ok, v = pcall(Input.GetButton, name)
    if ok then return v end
    return Input.GetKey(key)
end

local function buttonDown(name, key)
    local ok, v = pcall(Input.GetButtonDown, name)
    if ok then return v end
    return Input.GetKeyDown(key)
end

local function axis(name, fallback)
    local ok, v = pcall(Input.GetAxis, name)
    if ok then return v end
    return fallback()
end

-- ---- Locomotion: pure state, no scene access (unit tested on its own) ---------------------------

local Locomotion = {}
Locomotion.__index = Locomotion

-- `t` is the tuning table (the script's public fields).
function Locomotion.new(t)
    return setmetatable({
        t = t,
        vx = 0, vz = 0, -- intended planar velocity (SmoothDamp'd)
        vy = 0,
        velX = 0, velZ = 0, -- SmoothDamp velocities
        grounded = false,
        sinceGrounded = 1e9, -- seconds since the ground was last touched
        airTime = 0,
        jumpBuffer = 0, -- seconds left on a buffered jump press
        landTimer = 0,
        state = "Idle",
        crouching = false,
        crouch = 0, crouchVel = 0, -- 0..1 smoothed crouch amount
        running = false,
        phase = 0, -- stride phase in radians, advanced by distance
        phaseRate = 0,
        speed = 0, -- measured planar speed after the last move
        landing = 0, -- landing impulse (0..1) on the landing frame
        jumped = false,
        signal = {
            state = "Idle", speed = 0, speed01 = 0, walkSpeed = t.walkSpeed, runSpeed = t.runSpeed,
            grounded = false, airTime = 0, phase = 0, phaseRate = 0, landing = 0,
            strafe = 0, forward = 0, turnRate = 0, crouch = 0, pitch = 0, jumped = false,
        },
    }, Locomotion)
end

-- The speed the input asks for.
function Locomotion:targetSpeed(input)
    local t = self.t
    if self.crouching then return t.crouchSpeed end
    if self.running then return t.runSpeed end
    return t.walkSpeed
end

-- Step the velocities for this frame. `input` = { move = Vector3-like {x, z} in world space
-- (magnitude <= 1), run, crouch, jump (pressed this frame), canStand (headroom) }, `grounded`
-- = the Character Controller's isGrounded after the previous move. Returns the motion
-- (vx, vy, vz) in m/s to move by.
function Locomotion:step(input, dt, grounded)
    local t = self.t
    self.grounded = grounded
    self.jumped = false
    self.landing = 0
    if grounded then
        self.sinceGrounded = 0
        self.airTime = 0
    else
        self.sinceGrounded = self.sinceGrounded + dt
        self.airTime = self.airTime + dt
    end
    self.jumpBuffer = math.max(0, self.jumpBuffer - dt)
    if input.jump then self.jumpBuffer = t.jumpBuffer end
    self.landTimer = math.max(0, self.landTimer - dt)

    -- Crouch: enter on the ground, leave when there is headroom.
    if input.crouch and grounded then
        self.crouching = true
    elseif not input.crouch and input.canStand ~= false then
        self.crouching = false
    end
    self.crouch, self.crouchVel = smoothDamp(self.crouch, self.crouching and 1 or 0, self.crouchVel,
        t.crouchSmoothTime, dt)

    local mx, mz = input.move.x or 0, input.move.z or 0
    local moving = (mx * mx + mz * mz) > 0.01
    self.running = input.run and moving and not self.crouching and (input.forward or 0) > -0.1

    -- Planar velocity: SmoothDamp towards the target (slower in the air).
    local target = self:targetSpeed(input)
    local tx, tz = mx * target, mz * target
    local accelerating = (tx * tx + tz * tz) > (self.vx * self.vx + self.vz * self.vz)
    local smooth = accelerating and t.accelerationTime or t.decelerationTime
    if not grounded then smooth = smooth / math.max(0.05, t.airControl) end
    self.vx, self.velX = smoothDamp(self.vx, tx, self.velX, smooth, dt)
    self.vz, self.velZ = smoothDamp(self.vz, tz, self.velZ, smooth, dt)

    -- Vertical: a small downward push keeps isGrounded true on slopes and steps.
    if grounded and self.vy < 0 then self.vy = -2 end
    local canJump = (grounded or self.sinceGrounded <= t.coyoteTime) and self.vy <= 0
    if canJump and self.jumpBuffer > 0 and not self.crouching then
        self.vy = math.sqrt(2 * t.gravity * t.jumpHeight)
        self.jumpBuffer = 0
        self.sinceGrounded = 1e9 -- the coyote window is used up
        self.jumped = true
        self.state = "Jump"
    end
    self.vy = math.max(self.vy - t.gravity * dt, -t.terminalVelocity)
    return self.vx, self.vy, self.vz
end

-- After the move: landing detection, the stride phase (distance travelled) and the state.
-- `velocity` is the Character Controller's measured velocity.
function Locomotion:afterMove(nowGrounded, velocity, dt)
    local t = self.t
    local vx, vz = velocity.x or 0, velocity.z or 0
    local speed = math.sqrt(vx * vx + vz * vz)
    self.speed = speed
    if nowGrounded and not self.grounded then
        -- Landing: the impulse grows with the fall speed.
        local fall = -math.min(self.vy, velocity.y or 0)
        local impulse = clamp01((fall - t.landingMinSpeed) / math.max(0.01, t.landingMaxSpeed - t.landingMinSpeed))
        self.landing = impulse
        if impulse > 0 then self.landTimer = t.landDuration end
        self.vy = -2
    end
    self.grounded = nowGrounded
    if nowGrounded then
        self.sinceGrounded = 0
        self.airTime = 0
    end

    -- Stride phase: one stride (two steps, left and right) per 2π, advanced by the distance
    -- walked, so the bob stops exactly when the feet do.
    if nowGrounded then
        local run01 = clamp01((speed - t.walkSpeed) / math.max(0.01, t.runSpeed - t.walkSpeed))
        local step = t.walkStepLength + (t.runStepLength - t.walkStepLength) * run01
        local rate = speed / (2 * step) * 2 * math.pi
        self.phase = (self.phase + rate * dt) % (2 * math.pi)
        self.phaseRate = rate
    else
        self.phaseRate = 0
    end

    -- State.
    if not nowGrounded then
        if self.vy > 0.5 then
            self.state = "Jump"
        elseif self.airTime > 0.08 or self.state == "Jump" then
            self.state = "Fall"
        end
    elseif self.landTimer > 0 then
        self.state = "Land"
    elseif self.crouching then
        self.state = "Crouch"
    elseif speed < 0.2 then
        self.state = "Idle"
    elseif self.running and speed > t.walkSpeed + 0.3 then
        self.state = "Run"
    else
        self.state = "Walk"
    end
end

-- Fill the signal the head bob scripts read.
function Locomotion:publish(strafe, forward, turnRate, pitch)
    local s = self.signal
    s.state = self.state
    s.speed = self.speed
    s.speed01 = clamp01(self.speed / math.max(0.01, self.t.runSpeed))
    s.walkSpeed = self.t.walkSpeed
    s.runSpeed = self.t.runSpeed
    s.grounded = self.grounded
    s.airTime = self.airTime
    s.phase = self.phase
    s.phaseRate = self.phaseRate
    s.landing = self.landing
    s.strafe = strafe
    s.forward = forward
    s.turnRate = turnRate
    s.crouch = self.crouch
    s.pitch = pitch
    s.jumped = self.jumped
    return s
end

FirstPersonController.Locomotion = Locomotion

-- Mouse look: yaw turns the body, pitch the camera (clamped). Pure, for tests.
function FirstPersonController.look(yaw, pitch, mouseX, mouseY, sensitivity, pitchMin, pitchMax)
    yaw = (yaw + mouseX * sensitivity) % 360
    pitch = clamp(pitch - mouseY * sensitivity, pitchMin, pitchMax)
    return yaw, pitch
end

-- ---- the component --------------------------------------------------------------------------

-- First descendant with a Camera (depth first).
local function findCamera(t)
    local cam = t.gameObject:GetComponent("Camera")
    if cam ~= nil then return t, cam end
    for i = 0, t.childCount - 1 do
        local ct, c = findCamera(t:GetChild(i))
        if ct ~= nil then return ct, c end
    end
    return nil, nil
end

function FirstPersonController:Awake()
    self._loco = Locomotion.new(self)
    self.signal = self._loco.signal
end

function FirstPersonController:Start()
    self.cc = self.gameObject:GetComponent("CharacterController")
    if self.cc == nil then
        Debug.LogError("FirstPersonController needs a Character Controller on " .. self.gameObject.name)
    end
    self._yaw = self.transform.localEulerAngles.y
    self._pitch = 0
    self._smoothX, self._smoothY = 0, 0
    self._fovVel = 0
    for i = 0, self.transform.childCount - 1 do
        self._cameraT, self._camera = findCamera(self.transform:GetChild(i))
        if self._cameraT ~= nil then break end
    end
    if self._camera ~= nil then
        self._baseFov = self._camera.fieldOfView
    end
    if self.cc ~= nil then
        self._standHeight = self.cc.height
        local c = self.cc.center
        self._standCenter = { x = c.x, y = c.y, z = c.z }
        self._mask = Physics.DefaultRaycastLayers & ~(1 << self.gameObject.layer)
    end
    self._cursorLocked = false
    if self.lockCursor then
        Cursor.lockState = CursorLockMode.Locked
    end
    self._input = self:_findActions()
end

-- The actions of a Player Input on this object, or nil (Input Manager axes then).
function FirstPersonController:_findActions()
    local player = self.gameObject:GetComponent("PlayerInput")
    if player == nil then return nil end
    local ok, actions = pcall(function() return player.actions end)
    if not ok or actions == nil then return nil end
    local move = actions:FindAction("Move")
    if move == nil then return nil end
    return {
        player = player,
        move = move,
        look = actions:FindAction("Look"),
        jump = actions:FindAction("Jump"),
        sprint = actions:FindAction("Sprint"),
        crouch = actions:FindAction("Crouch"),
    }
end

-- Is the Player Input's movement map switched off (a menu is open)?
function FirstPersonController:_inputPaused()
    return self._input ~= nil and not self._input.move.enabled
end

-- Esc releases the cursor, a click on the game takes it again.
function FirstPersonController:_updateCursor()
    if self:_inputPaused() then
        self._cursorLocked = false
        return
    end
    if not self.lockCursor then
        self._cursorLocked = true
        return
    end
    local locked = Cursor.lockState == CursorLockMode.Locked
    if locked and Input.GetKeyDown("escape") then
        Cursor.lockState = CursorLockMode.None
        locked = false
    elseif not locked and Input.GetMouseButtonDown(0) then
        Cursor.lockState = CursorLockMode.Locked
        locked = true
    end
    self._cursorLocked = locked
end

-- Returns the yaw turned this frame in degrees per second.
function FirstPersonController:_updateLook(dt)
    local mx, my = 0, 0
    local look = self._input and self._input.look
    if look ~= nil then
        -- The mouse binding is scaled like the Mouse X / Y axes; a stick turns at a rate.
        local v = look:ReadValue()
        if self._input.player.currentControlScheme == "Gamepad" then
            local k = self.stickLookSpeed * dt / math.max(0.001, self.mouseSensitivity)
            mx, my = v.x * k, v.y * k
        elseif self._cursorLocked then
            mx, my = v.x, v.y
        end
    elseif self._cursorLocked then
        mx = axis("Mouse X", function() return Input.mouseDelta.x * 0.1 end)
        my = axis("Mouse Y", function() return Input.mouseDelta.y * 0.1 end)
    end
    if self.mouseSmoothing > 0 then
        local k = 1 - math.exp(-dt / self.mouseSmoothing)
        self._smoothX = self._smoothX + (mx - self._smoothX) * k
        self._smoothY = self._smoothY + (my - self._smoothY) * k
        mx, my = self._smoothX, self._smoothY
    end
    local before = self._yaw
    self._yaw, self._pitch = FirstPersonController.look(self._yaw, self._pitch, mx, my,
        self.mouseSensitivity, self.pitchMin, self.pitchMax)
    self.transform.localEulerAngles = Vector3.new(0, self._yaw, 0)
    if self._cameraT ~= nil then
        self._cameraT.localEulerAngles = Vector3.new(self._pitch, 0, 0)
    end
    local turned = (self._yaw - before + 540) % 360 - 180
    return turned / dt
end

-- Is there room to stand up from a crouch?
function FirstPersonController:_canStand()
    if self.cc == nil or not self._loco.crouching then return true end
    local p = self.transform.position
    local radius = self.cc.radius * 0.8
    local top = p + Vector3.up * (self.crouchHeight - radius + 0.05)
    local room = self._standHeight - self.crouchHeight
    return Physics.SphereCast(top, radius, Vector3.up, room, self._mask) == nil
end

-- The capsule follows the crouch amount; the feet stay on the floor.
function FirstPersonController:_applyCrouch()
    if self.cc == nil then return end
    local h = self._standHeight + (self.crouchHeight - self._standHeight) * self._loco.crouch
    if math.abs(h - self.cc.height) > 0.002 then
        self.cc.height = h
        local c = self._standCenter
        self.cc.center = Vector3.new(c.x, c.y - (self._standHeight - h) * 0.5, c.z)
    end
end

function FirstPersonController:Update()
    local dt = Time.deltaTime
    if dt <= 0 or self.cc == nil then return end
    local loco = self._loco
    self:_updateCursor()
    local turnRate = self:_updateLook(dt)

    local h, v, run, crouch, jump
    local actions = self._input
    if actions ~= nil then
        local m = actions.move:ReadValue()
        h, v = m.x, m.y
        run = actions.sprint ~= nil and actions.sprint:IsPressed()
        crouch = actions.crouch ~= nil and actions.crouch:IsPressed()
        jump = actions.jump ~= nil and actions.jump:WasPressedThisFrame()
    else
        h = axis("Horizontal", function() return 0 end)
        v = axis("Vertical", function() return 0 end)
        run = button("Run", "left shift")
        crouch = button("Crouch", "left ctrl")
        jump = buttonDown("Jump", "space")
    end
    local wish = Vector3.new(h, 0, v)
    if wish.magnitude > 1 then wish = wish.normalized end
    local dir = self.transform.right * wish.x + self.transform.forward * wish.z
    local input = {
        move = { x = dir.x, z = dir.z },
        forward = v,
        run = run,
        crouch = crouch,
        jump = jump,
        canStand = self:_canStand(),
    }
    local vx, vy, vz = loco:step(input, dt, self.cc.isGrounded)
    self.cc:Move(Vector3.new(vx, vy, vz) * dt)
    loco:afterMove(self.cc.isGrounded, self.cc.velocity, dt)
    self:_applyCrouch()

    -- Sprint FOV kick.
    if self._camera ~= nil then
        local target = self._baseFov + (loco.state == "Run" and self.sprintFov or 0)
        local fov
        fov, self._fovVel = smoothDamp(self._camera.fieldOfView, target, self._fovVel, self.fovSmoothTime, dt)
        self._camera.fieldOfView = fov
    end
    loco:publish(h, v, turnRate, self._pitch)
end

-- Crates and barrels get a shove.
function FirstPersonController:OnControllerColliderHit(hit)
    local body = hit.rigidbody
    if body == nil or hit.moveDirection.y < -0.3 then return end
    body.velocity = Vector3.new(hit.moveDirection.x, 0, hit.moveDirection.z) * self.pushPower
end

return FirstPersonController
