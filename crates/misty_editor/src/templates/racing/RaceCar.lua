-- RaceCar: an arcade car on four Wheel Colliders (all-wheel drive).
-- W/S or the arrow keys: throttle and brake (S reverses once stopped). A/D: steer.
-- Space: handbrake. Left Shift: boost. R: back onto the track.
--
-- Arcade help on top of the wheel physics: grippy tyres, downforce that grows with speed,
-- a top speed, a boost meter, and a reset when the car flips over or stays off the track.
-- RaceSettings.lua changes the car, its mass, grip and top speed while playing
-- (SetModel, mass, grip, topSpeed below).
local RaceCar = {
    -- The car models under this object (shown one at a time) and their tuning.
    -- torque: N·m per wheel, mass: kg, grip: tyre grip multiplier, topSpeed: km/h.
    models = {
        { object = "Model Sport", label = "Sport", torque = 950, mass = 1150, grip = 1.6, topSpeed = 230 },
        { object = "Model Muscle", label = "Muscle", torque = 1250, mass = 1500, grip = 1.35, topSpeed = 250 },
        { object = "Model Kart", label = "Kart", torque = 600, mass = 750, grip = 1.9, topSpeed = 190 },
    },
    model = 1,
    brakeTorque = 3000,
    handbrakeTorque = 7000,
    maxSteerAngle = 30,
    -- Steering at top speed (degrees): less lock keeps the car stable when fast.
    highSpeedSteerAngle = 7,
    steerSpeed = 160, -- degrees per second
    downforce = 3.0, -- newtons per (m/s)² per tonne
    rollDamping = 6, -- per second: how quickly body roll and pitch settle
    yawDamping = 0.6, -- per second
    boostAcceleration = 9, -- m/s² while boosting
    boostDuration = 3, -- seconds of boost in a full meter
    boostRecharge = 7, -- seconds to refill an empty meter
    -- Off the track the grass slows the car (extra drag per second).
    grassDrag = 0.6,
    -- Seconds upside down or off the track before the car is put back.
    flippedTime = 1.5,
    offTrackTime = 4,
    -- How far from the road's centre still counts as on the track (metres).
    trackHalfWidth = 9,
    track = "Track",
    wheels = { "Wheel FL", "Wheel FR", "Wheel RL", "Wheel RR" },
    speedText = "Speed Text",
    boostText = "Boost Text",
    messageText = "Message Text",
}

local function find(name)
    local go = GameObject.Find(name)
    if go == nil then
        Debug.LogError("RaceCar: no object called " .. name)
    end
    return go
end

local function text(name)
    local go = GameObject.Find(name)
    if go ~= nil then
        return go:GetComponent("Text")
    end
    return nil
end

function RaceCar:Start()
    self.body = self.gameObject:GetComponent("Rigidbody")
    self.wheelColliders = {}
    for i, name in ipairs(self.wheels) do
        local go = find(name)
        if go ~= nil then
            self.wheelColliders[i] = go:GetComponent("WheelCollider")
        end
    end
    local track = GameObject.Find(self.track)
    if track ~= nil then
        self.spline = track:GetComponent("SplineContainer")
    end
    self.speedLabel = text(self.speedText)
    self.boostLabel = text(self.boostText)
    self.messageLabel = text(self.messageText)
    self.boost = 1
    self.boosting = false
    self._steer = 0
    self._flipped = 0
    self._offTrack = 0
    self._message = 0
    self:SetModel(self.model)
end

-- Show car model `index` and take over its tuning (mass, grip, top speed, torque).
function RaceCar:SetModel(index)
    local count = #self.models
    index = ((index - 1) % count) + 1
    self.model = index
    for i, m in ipairs(self.models) do
        local go = GameObject.Find(m.object)
        if go ~= nil then
            go:SetActive(i == index)
        end
    end
    local m = self.models[index]
    self.torque = m.torque
    self.mass = m.mass
    self.grip = m.grip
    self.topSpeed = m.topSpeed
    self:ApplyTuning()
end

function RaceCar:ModelName()
    return self.models[self.model].label
end

-- Push mass and grip into the physics (called after changing them).
function RaceCar:ApplyTuning()
    self.body.mass = self.mass
    for _, w in ipairs(self.wheelColliders) do
        w.forwardStiffness = self.grip
        w.sidewaysStiffness = self.grip
    end
end

local function key(a, b)
    return Input.GetKey(a) or Input.GetKey(b)
end

-- Distance from the road's centre line, and the nearest point and its spline position (0..1).
function RaceCar:TrackPosition()
    if self.spline == nil then
        return 0, self.transform.position, 0
    end
    local point, t = self.spline:GetNearestPoint(self.transform.position)
    local d = self.transform.position - point
    return Vector3.new(d.x, 0, d.z).magnitude, point, t
end

function RaceCar:FixedUpdate()
    local dt = Time.fixedDeltaTime
    local velocity = self.body.velocity
    local forward = self.transform.forward
    local speed = Vector3.Dot(velocity, forward)
    local top = self.topSpeed / 3.6

    -- Steering eases towards the keys, with less lock at speed.
    local input = 0
    if key("a", "left") then input = input - 1 end
    if key("d", "right") then input = input + 1 end
    local fast = Mathf.Clamp01(math.abs(speed) / top)
    local limit = Mathf.Lerp(self.maxSteerAngle, self.highSpeedSteerAngle, fast)
    local target = input * limit
    local step = self.steerSpeed * dt
    if math.abs(target - self._steer) <= step then
        self._steer = target
    elseif target > self._steer then
        self._steer = self._steer + step
    else
        self._steer = self._steer - step
    end

    -- Throttle fades out towards the top speed; S brakes, then reverses.
    local motor, brake = 0, 0
    if key("w", "up") then
        if speed < -1 then
            brake = self.brakeTorque
        else
            motor = self.torque * Mathf.Clamp01(1 - speed / top)
        end
    elseif key("s", "down") then
        if speed > 1 then
            brake = self.brakeTorque
        else
            motor = -self.torque * 0.6 * Mathf.Clamp01(1 + speed / 12)
        end
    else
        brake = 60 -- engine braking
    end
    local handbrake = Input.GetKey("space")
    for i, w in ipairs(self.wheelColliders) do
        if i <= 2 then
            w.steerAngle = self._steer
        end
        w.motorTorque = motor
        if handbrake and i > 2 then
            w.motorTorque = 0
            w.brakeTorque = self.handbrakeTorque
        else
            w.brakeTorque = brake
        end
    end

    local grounded = 0
    for _, w in ipairs(self.wheelColliders) do
        if w.isGrounded then grounded = grounded + 1 end
    end
    if grounded > 0 then
        -- Downforce: more grip the faster the car goes.
        local push = self.downforce * self.mass / 1000 * speed * speed
        self.body:AddForce(self.transform.up * -push)
        -- Arcade stability: damp the roll and the pitch strongly (the car stays flat in
        -- corners and over bumps) and the yaw a little (it doesn't spin out on its own).
        local spin = self.body.angularVelocity
        local roll = Vector3.Dot(spin, forward)
        local pitch = Vector3.Dot(spin, self.transform.right)
        local yaw = Vector3.Dot(spin, self.transform.up)
        spin = spin - forward * (roll * self.rollDamping * dt)
            - self.transform.right * (pitch * self.rollDamping * dt)
            - self.transform.up * (yaw * self.yawDamping * dt)
        self.body.angularVelocity = spin
    end

    -- Boost: a push forward while the meter lasts (also past the top speed).
    self.boosting = Input.GetKey("left shift") and self.boost > 0 and grounded > 0
    if self.boosting then
        self.body:AddForce(forward * self.boostAcceleration, "Acceleration")
        self.boost = math.max(0, self.boost - dt / self.boostDuration)
    elseif not Input.GetKey("left shift") then
        self.boost = math.min(1, self.boost + dt / self.boostRecharge)
    end

    -- The grass slows the car down.
    local off = self:TrackPosition()
    if off > self.trackHalfWidth and grounded > 0 then
        self.body.velocity = velocity * (1 - self.grassDrag * dt)
    end
end

-- Put the car back on the road at the nearest point, facing along the track.
function RaceCar:ResetToTrack()
    local _, point, t = self:TrackPosition()
    local yaw = self.transform.eulerAngles.y
    if self.spline ~= nil then
        local dir = self.spline:EvaluateTangent(t)
        local flat = Vector3.new(dir.x, 0, dir.z)
        if flat.magnitude > 0.001 then
            yaw = math.atan(flat.x, flat.z) * Mathf.Rad2Deg
        end
    end
    self.transform.position = Vector3.new(point.x, point.y + 1.2, point.z)
    self.transform.eulerAngles = Vector3.new(0, yaw, 0)
    self.body.velocity = Vector3.zero
    self.body.angularVelocity = Vector3.zero
    self._flipped = 0
    self._offTrack = 0
    self:Say("Back on track")
end

function RaceCar:Say(message)
    if self.messageLabel ~= nil then
        self.messageLabel.text = message
    end
    self._message = 2
end

function RaceCar:Update()
    local dt = Time.deltaTime
    if self.transform.up.y < 0.3 then
        self._flipped = self._flipped + dt
    else
        self._flipped = 0
    end
    local off = self:TrackPosition()
    if off > self.trackHalfWidth + 6 then
        self._offTrack = self._offTrack + dt
    else
        self._offTrack = 0
    end
    if Input.GetKeyDown("r") or self._flipped > self.flippedTime
        or self._offTrack > self.offTrackTime then
        self:ResetToTrack()
    end

    if self._message > 0 then
        self._message = self._message - dt
        if self._message <= 0 and self.messageLabel ~= nil then
            self.messageLabel.text = ""
        end
    end
    if self.speedLabel ~= nil then
        local kmh = math.floor(self.body.velocity.magnitude * 3.6 + 0.5)
        local shown = kmh .. " km/h"
        if shown ~= self._shownSpeed then
            self.speedLabel.text = shown
            self._shownSpeed = shown
        end
    end
    if self.boostLabel ~= nil then
        local bars = math.floor(self.boost * 10 + 0.5)
        local shown = "BOOST " .. string.rep("|", bars) .. string.rep(".", 10 - bars)
        if shown ~= self._shownBoost then
            self.boostLabel.text = shown
            self._shownBoost = shown
        end
    end
end

return RaceCar
