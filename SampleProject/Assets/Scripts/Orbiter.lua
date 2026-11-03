-- Orbiter: moves the object around an ellipse. With a 3D Audio Source on it you hear it pan
-- around the listener, fade with distance and shift pitch (Doppler) as it comes and goes.
local Orbiter = {
    center = Vector3.new(0, 1, 1),
    radiusX = 7,
    radiusZ = 5,
    speed = 0.6,                     -- radians per second
}

function Orbiter:Update()
    local a = Time.time * self.speed
    self.transform.position = Vector3.new(
        self.center.x + Mathf.Cos(a) * self.radiusX,
        self.center.y,
        self.center.z + Mathf.Sin(a) * self.radiusZ)
end

return Orbiter
