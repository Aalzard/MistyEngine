-- PathRunner: jogs around an oval track, facing where it is going, with a little bounce.
local PathRunner = {
    radiusX = 11,
    radiusZ = 7,
    speed = 0.3,        -- radians per second around the oval
    bounce = 0.08,      -- metres of bounce per stride
}

function PathRunner:Start()
    self._height = self.transform.position.y
end

-- Point on the oval at angle `a` (starts on the near side, heading +X).
function PathRunner:At(a)
    return Vector3.new(math.cos(a) * self.radiusX, self._height, math.sin(a) * self.radiusZ)
end

function PathRunner:Update()
    local a = Time.time * self.speed - math.pi / 2
    local p = self:At(a)
    self.transform:LookAt(self:At(a + 0.05))
    p.y = self._height + math.abs(math.sin(Time.time * 7)) * self.bounce
    self.transform.position = p
end

return PathRunner
