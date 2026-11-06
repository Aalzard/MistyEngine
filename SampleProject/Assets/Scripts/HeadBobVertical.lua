-- HeadBobVertical: moves this object up and down with the footsteps. Put it on a child of a
-- FirstPersonController (Player > HeadBobVertical > HeadBobHorizontal > Camera) and it reads the
-- controller's locomotion signal: two bounces per stride (sin(2·phase), the phase advances with
-- the distance walked so the bob stops with the feet), a spring-damper dip on landing, and a
-- slow breathing sine while idle. Amplitudes blend with the speed through SmoothDamp, so
-- starting, stopping, walking into a run and leaving the ground never jump; the stored base
-- pose only ever gets an offset added.
local HeadBobVertical = {
    walkAmplitude = 0.03, -- metres at walking speed
    runAmplitude = 0.05, -- metres at running speed
    blendTime = 0.25, -- seconds to blend the amplitude in and out
    landingDip = 0.10, -- metres of dip for a full landing impulse
    landingFrequency = 7, -- spring stiffness (cycles per second)
    landingDamping = 0.9, -- damping ratio (1 = critically damped)
    breathingAmplitude = 0.004, -- metres while idle
    breathingRate = 0.3, -- breaths per second
    crouchDrop = 0.6, -- metres the eyes lower when fully crouched
}

local function clamp01(v)
    if v < 0 then return 0 end
    if v > 1 then return 1 end
    return v
end

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

-- Damped spring (position, velocity) integrated in small sub-steps so a long frame stays stable.
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

-- The locomotion signal of the FirstPersonController on this object or a parent.
local function findSignal(t)
    while t ~= nil do
        local c = t.gameObject:GetComponent("FirstPersonController")
        if c ~= nil and c.signal ~= nil then return c.signal end
        t = t.parent
    end
    return nil
end

-- Bob amplitude for a planar speed: fades in over the first half of walking speed, then
-- blends from the walk to the run amplitude.
function HeadBobVertical.amplitude(self, speed, walkSpeed, runSpeed)
    local fade = clamp01(speed / math.max(0.01, walkSpeed * 0.5))
    local run01 = clamp01((speed - walkSpeed) / math.max(0.01, runSpeed - walkSpeed))
    return (self.walkAmplitude + (self.runAmplitude - self.walkAmplitude) * run01) * fade
end

-- State without a scene (unit tests call this, then Evaluate).
function HeadBobVertical:_init()
    self._amp, self._ampVel = 0, 0
    self._springPos, self._springVel = 0, 0
    self._idle, self._idleVel = 0, 0
    self._breath = 0
    self._last = 0
    self.offset = 0
    self.velocity = 0
end

-- One frame of the bob from a locomotion signal; returns the vertical offset in metres.
function HeadBobVertical:Evaluate(sig, dt)
    local grounded = sig ~= nil and sig.grounded
    local speed = sig ~= nil and sig.speed or 0
    local target = 0
    if grounded then target = HeadBobVertical.amplitude(self, speed, sig.walkSpeed or 4, sig.runSpeed or 7) end
    self._amp, self._ampVel = smoothDamp(self._amp, target, self._ampVel, self.blendTime, dt)
    local phase = sig ~= nil and sig.phase or 0
    local bob = self._amp * math.sin(2 * phase)

    if sig ~= nil and (sig.landing or 0) > 0 then
        -- A velocity kick that peaks at about landingDip · impulse.
        local omega = 2 * math.pi * self.landingFrequency
        self._springVel = self._springVel - sig.landing * self.landingDip * omega * 2.718
    end
    self._springPos, self._springVel = spring(self._springPos, self._springVel, self.landingFrequency,
        self.landingDamping, dt)

    local idle = (grounded and speed < 0.2) and 1 or 0
    self._idle, self._idleVel = smoothDamp(self._idle, idle, self._idleVel, 0.6, dt)
    self._breath = self._breath + dt
    local breath = self._idle * self.breathingAmplitude * math.sin(2 * math.pi * self.breathingRate * self._breath)

    local crouch = -(sig ~= nil and sig.crouch or 0) * self.crouchDrop
    local offset = bob + self._springPos + breath + crouch
    self.velocity = (offset - self._last) / dt
    self._last = offset
    self.offset = offset
    if sig ~= nil then
        sig.verticalBob = offset
        sig.verticalBobVelocity = self.velocity
    end
    return offset
end

function HeadBobVertical:Start()
    self:_init()
    local p = self.transform.localPosition
    self._base = { x = p.x, y = p.y, z = p.z }
    self._signal = findSignal(self.transform)
end

function HeadBobVertical:LateUpdate()
    local dt = Time.deltaTime
    if dt <= 0 then return end
    if self._signal == nil then self._signal = findSignal(self.transform) end
    local offset = self:Evaluate(self._signal, dt)
    local b = self._base
    self.transform.localPosition = Vector3.new(b.x, b.y + offset, b.z)
end

return HeadBobVertical
