-- Walker: carries the listener (this camera) from the meadow into the cave and back over
-- `period` seconds. Holding W or S walks by hand instead.
local Walker = {
    near = -12,
    far = 9,
    period = 24,
    speed = 4,
    manual = false,
    z = -12,
}

function Walker:Update()
    if Input.GetKey("w") or Input.GetKey("s") then self.manual = true end
    if self.manual then
        if Input.GetKey("w") then self.z = math.min(self.z + self.speed * Time.deltaTime, self.far) end
        if Input.GetKey("s") then self.z = math.max(self.z - self.speed * Time.deltaTime, self.near) end
    else
        local k = 0.5 - 0.5 * Mathf.Cos(Time.time / self.period * 2 * math.pi)
        self.z = self.near + (self.far - self.near) * k
    end
    local p = self.transform.position
    self.transform.position = Vector3.new(p.x, p.y, self.z)
end

return Walker
