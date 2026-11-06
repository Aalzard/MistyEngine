-- HeadBobHorizontal: sways this object sideways with the footsteps, one sway per stride
-- (sin(phase): left foot, right foot), plus a small lean into a strafe. Reads the
-- FirstPersonController's locomotion signal from a parent, blends its amplitude with the speed
-- through SmoothDamp and adds the offset to the stored base pose.
local HeadBobHorizontal = {
    walkAmplitude = 0.02, -- metres at walking speed
    runAmplitude = 0.035, -- metres at running speed
    blendTime = 0.25, -- seconds to blend the amplitude in and out
    strafeLean = 0.03, -- metres the head shifts towards a strafe
    strafeSmoothTime = 0.2,
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

local function findSignal(t)
    while t ~= nil do
        local c = t.gameObject:GetComponent("FirstPersonController")
        if c ~= nil and c.signal ~= nil then return c.signal end
        t = t.parent
    end
    return nil
end

function HeadBobHorizontal.amplitude(self, speed, walkSpeed, runSpeed)
    local fade = clamp01(speed / math.max(0.01, walkSpeed * 0.5))
    local run01 = clamp01((speed - walkSpeed) / math.max(0.01, runSpeed - walkSpeed))
    return (self.walkAmplitude + (self.runAmplitude - self.walkAmplitude) * run01) * fade
end

function HeadBobHorizontal:_init()
    self._amp, self._ampVel = 0, 0
    self._lean, self._leanVel = 0, 0
    self.offset = 0
end

-- One frame of sway from a locomotion signal; returns the sideways offset in metres.
function HeadBobHorizontal:Evaluate(sig, dt)
    local grounded = sig ~= nil and sig.grounded
    local speed = sig ~= nil and sig.speed or 0
    local target = 0
    if grounded then target = HeadBobHorizontal.amplitude(self, speed, sig.walkSpeed or 4, sig.runSpeed or 7) end
    self._amp, self._ampVel = smoothDamp(self._amp, target, self._ampVel, self.blendTime, dt)
    local phase = sig ~= nil and sig.phase or 0
    local sway = self._amp * math.sin(phase)

    local lean = 0
    if grounded then lean = (sig.strafe or 0) * self.strafeLean end
    self._lean, self._leanVel = smoothDamp(self._lean, lean, self._leanVel, self.strafeSmoothTime, dt)

    self.offset = sway + self._lean
    if sig ~= nil then sig.horizontalBob = self.offset end
    return self.offset
end

function HeadBobHorizontal:Start()
    self:_init()
    local p = self.transform.localPosition
    self._base = { x = p.x, y = p.y, z = p.z }
    self._signal = findSignal(self.transform)
end

function HeadBobHorizontal:LateUpdate()
    local dt = Time.deltaTime
    if dt <= 0 then return end
    if self._signal == nil then self._signal = findSignal(self.transform) end
    local offset = self:Evaluate(self._signal, dt)
    local b = self._base
    self.transform.localPosition = Vector3.new(b.x + offset, b.y, b.z)
end

return HeadBobHorizontal
