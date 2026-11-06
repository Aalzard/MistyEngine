-- HeadBobRotation: on the Camera of a First Person Player. Adds a stride roll (sin(phase),
-- scaled by the speed), a lean into strafes, a counter-roll on fast mouse turns, a pitch bob
-- tied to the vertical bob's velocity and a landing pitch kick that springs back. Every term is
-- SmoothDamp'd and clamped, and only added on top of the pitch the FirstPersonController sets
-- (mouse look owns the base pose; this script never fights it).
local HeadBobRotation = {
    walkRoll = 0.8, -- degrees of roll per stride at walking speed
    runRoll = 1.6, -- ... at running speed
    blendTime = 0.25,
    strafeTilt = 1.5, -- degrees of lean into a full strafe
    turnTilt = 0.004, -- degrees of counter-roll per degree/second of mouse turn
    maxTurnTilt = 2,
    pitchBob = 6, -- degrees of pitch per m/s of vertical bob velocity
    maxPitchBob = 2,
    landingKick = 3, -- degrees of downward pitch for a full landing impulse
    landingFrequency = 6,
    landingDamping = 0.9,
    maxRoll = 6, -- clamps on the final offsets
    maxPitch = 5,
    smoothTime = 0.08, -- final smoothing of the roll and pitch offsets
}

local function clamp(v, lo, hi)
    if v < lo then return lo end
    if v > hi then return hi end
    return v
end

local function clamp01(v) return clamp(v, 0, 1) end

local function smoothDamp(current, target, velocity, smoothTime, dt)
    smoothTime = math.max(0.0001, smoothTime)
    local omega = 2 / smoothTime
    local x = omega * dt
    local exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x)
    local change = current - target
    local temp = (velocity + omega * change) * dt
    velocity = (velocity - omega * temp) * exp
    local out = target + (change + temp) * exp
    if (target - current > 0) == (out > target) then
        out = target
        velocity = (out - target) / dt
    end
    return out, velocity
end

local function spring(pos, vel, frequency, damping, dt)
    local omega = 2 * math.pi * frequency
    local steps = math.max(1, math.ceil(dt / (1 / 240)))
    local h = dt / steps
    for _ = 1, steps do
        vel = vel + (-omega * omega * pos - 2 * damping * omega * vel) * h
        pos = pos + vel * h
    end
    return pos, vel
end

local function findSignal(t)
    while t ~= nil do
        local c = t.gameObject:GetComponent("FirstPersonController")
        if c ~= nil and c.signal ~= nil then return c.signal end
        t = t.parent
    end
    return nil
end

function HeadBobRotation.rollAmplitude(self, speed, walkSpeed, runSpeed)
    local fade = clamp01(speed / math.max(0.01, walkSpeed * 0.5))
    local run01 = clamp01((speed - walkSpeed) / math.max(0.01, runSpeed - walkSpeed))
    return (self.walkRoll + (self.runRoll - self.walkRoll) * run01) * fade
end

function HeadBobRotation:_init()
    self._amp, self._ampVel = 0, 0
    self._strafe, self._strafeVel = 0, 0
    self._turn, self._turnVel = 0, 0
    self._springPos, self._springVel = 0, 0
    self._roll, self._rollVel = 0, 0
    self._pitch, self._pitchVel = 0, 0
    self.pitchOffset = 0
    self.rollOffset = 0
end

-- One frame from a locomotion signal; returns the pitch and roll offsets in degrees.
function HeadBobRotation:Evaluate(sig, dt)
    local grounded = sig ~= nil and sig.grounded
    local speed = sig ~= nil and sig.speed or 0
    local target = 0
    if grounded then target = HeadBobRotation.rollAmplitude(self, speed, sig.walkSpeed or 4, sig.runSpeed or 7) end
    self._amp, self._ampVel = smoothDamp(self._amp, target, self._ampVel, self.blendTime, dt)
    local phase = sig ~= nil and sig.phase or 0
    local strideRoll = self._amp * math.sin(phase)

    -- Lean into a strafe (negative roll = tilt to the right).
    local strafe = 0
    if grounded then strafe = -(sig.strafe or 0) * self.strafeTilt end
    self._strafe, self._strafeVel = smoothDamp(self._strafe, strafe, self._strafeVel, 0.2, dt)

    -- Counter-roll on a fast turn, like the head lagging the body.
    local turn = 0
    if sig ~= nil then turn = clamp(-(sig.turnRate or 0) * self.turnTilt, -self.maxTurnTilt, self.maxTurnTilt) end
    self._turn, self._turnVel = smoothDamp(self._turn, turn, self._turnVel, 0.1, dt)

    -- Pitch follows the vertical bob's velocity (eyes compensate a rising head by looking down).
    local vBob = 0
    if sig ~= nil then
        if sig.verticalBobVelocity ~= nil then
            vBob = sig.verticalBobVelocity
        elseif grounded then
            -- No HeadBobVertical in the chain: the derivative of its bob.
            vBob = self._amp * 0.03 * 2 * math.cos(2 * phase) * (sig.phaseRate or 0)
        end
    end
    local pitchBob = clamp(-vBob * self.pitchBob, -self.maxPitchBob, self.maxPitchBob)

    if sig ~= nil and (sig.landing or 0) > 0 then
        local omega = 2 * math.pi * self.landingFrequency
        self._springVel = self._springVel + sig.landing * self.landingKick * omega * 2.718
    end
    self._springPos, self._springVel = spring(self._springPos, self._springVel, self.landingFrequency,
        self.landingDamping, dt)

    -- The springs are smooth already; the rest gets a final SmoothDamp, then the clamps.
    local roll = clamp(strideRoll + self._strafe + self._turn, -self.maxRoll, self.maxRoll)
    self._roll, self._rollVel = smoothDamp(self._roll, roll, self._rollVel, self.smoothTime, dt)
    self._pitch, self._pitchVel = smoothDamp(self._pitch, pitchBob, self._pitchVel, self.smoothTime, dt)
    self.rollOffset = clamp(self._roll, -self.maxRoll, self.maxRoll)
    self.pitchOffset = clamp(self._pitch + self._springPos, -self.maxPitch, self.maxPitch)
    return self.pitchOffset, self.rollOffset
end

function HeadBobRotation:Start()
    self:_init()
    local e = self.transform.localEulerAngles
    self._base = { x = e.x, y = e.y, z = e.z }
    self._signal = findSignal(self.transform)
end

function HeadBobRotation:LateUpdate()
    local dt = Time.deltaTime
    if dt <= 0 then return end
    if self._signal == nil then self._signal = findSignal(self.transform) end
    local pitch, roll = self:Evaluate(self._signal, dt)
    local b = self._base
    -- The controller's mouse look sets the pitch each Update; without one the base pitch stays.
    local basePitch = self._signal ~= nil and self._signal.pitch or b.x
    self.transform.localEulerAngles = Vector3.new(basePitch + pitch, b.y, b.z + roll)
end

return HeadBobRotation
