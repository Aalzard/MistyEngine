-- CameraOrbit: circles a point at a fixed height, always looking at it.
local CameraOrbit = {
    center = Vector3.new(0, 0, 0),
    radius = 70,
    height = 30,
    speed = 0.15,                    -- radians per second
}

function CameraOrbit:Update()
    local a = Time.time * self.speed
    self.transform.position = Vector3.new(
        self.center.x + Mathf.Sin(a) * self.radius,
        self.center.y + self.height,
        self.center.z - Mathf.Cos(a) * self.radius)
    self.transform:LookAt(self.center)
end

return CameraOrbit
