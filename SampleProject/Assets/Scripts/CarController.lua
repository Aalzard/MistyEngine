-- CarController: W/S (or the arrow keys) throttle and brake, S reverses once stopped,
-- A/D steer, Space pulls the handbrake (rear wheels), R puts the car back on its wheels.
-- Drives the Wheel Colliders named below (rear-wheel drive) and shows the speed in a Text.
local CarController = {
    motorTorque = 1100,
    reverseTorque = 700,
    brakeTorque = 2500,
    handbrakeTorque = 6000,
    maxSteerAngle = 32,
    -- Steering eases towards the keys at this many degrees per second.
    steerSpeed = 140,
    -- Less steering at speed (degrees at 30 m/s).
    highSpeedSteerAngle = 10,
    frontLeft = "Wheel FL",
    frontRight = "Wheel FR",
    rearLeft = "Wheel RL",
    rearRight = "Wheel RR",
    speedText = "Speed Text",
}

local function wheel(name)
    local go = GameObject.Find(name)
    if go == nil then
        Debug.LogError("CarController: no wheel called " .. name)
        return nil
    end
    return go:GetComponent("WheelCollider")
end

function CarController:Start()
    self.body = self.gameObject:GetComponent("Rigidbody")
    self.front = { wheel(self.frontLeft), wheel(self.frontRight) }
    self.rear = { wheel(self.rearLeft), wheel(self.rearRight) }
    self._steer = 0
    local text = GameObject.Find(self.speedText)
    if text ~= nil then
        self.label = text:GetComponent("Text")
    end
end

local function key(a, b)
    return Input.GetKey(a) or Input.GetKey(b)
end

function CarController:FixedUpdate()
    local dt = Time.fixedDeltaTime
    local speed = Vector3.Dot(self.body.velocity, self.transform.forward)

    -- Steering: ease towards the keys, less of it at speed.
    local input = 0
    if key("a", "left") then input = input - 1 end
    if key("d", "right") then input = input + 1 end
    local t = Mathf.Clamp(math.abs(speed) / 30, 0, 1)
    local limit = Mathf.Lerp(self.maxSteerAngle, self.highSpeedSteerAngle, t)
    local target = input * limit
    local step = self.steerSpeed * dt
    if math.abs(target - self._steer) <= step then
        self._steer = target
    elseif target > self._steer then
        self._steer = self._steer + step
    else
        self._steer = self._steer - step
    end

    -- Throttle and brake: S brakes while rolling forward, then reverses (W likewise).
    local motor, brake = 0, 0
    if key("w", "up") then
        if speed < -0.5 then brake = self.brakeTorque else motor = self.motorTorque end
    elseif key("s", "down") then
        if speed > 0.5 then brake = self.brakeTorque else motor = -self.reverseTorque end
    else
        -- Engine braking, so the car slows down when coasting.
        brake = 40
    end
    local handbrake = Input.GetKey("space")

    for _, w in ipairs(self.front) do
        w.steerAngle = self._steer
        w.motorTorque = 0
        w.brakeTorque = brake
    end
    for _, w in ipairs(self.rear) do
        w.motorTorque = motor
        if handbrake then
            w.motorTorque = 0
            w.brakeTorque = self.handbrakeTorque
        else
            w.brakeTorque = brake
        end
    end
end

function CarController:Update()
    if Input.GetKeyDown("r") then
        -- Back on the wheels, facing the same way.
        local yaw = self.transform.eulerAngles.y
        self.transform.position = self.transform.position + Vector3.up * 1.5
        self.transform.eulerAngles = Vector3.new(0, yaw, 0)
        self.body.velocity = Vector3.zero
        self.body.angularVelocity = Vector3.zero
    end
    if self.label ~= nil then
        local kmh = math.floor(self.body.velocity.magnitude * 3.6 + 0.5)
        local text = kmh .. " km/h"
        if text ~= self._shown then
            self.label.text = text
            self._shown = text
        end
    end
end

return CarController
