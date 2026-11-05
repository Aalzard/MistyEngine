-- Bobber (Example Tools package): floats the object up and down around where it starts.
local Bobber = { height = 0.25, speed = 2 }

function Bobber:Start()
    self._base = self.transform.position
    self._t = 0
end

function Bobber:Update()
    self._t = self._t + Time.deltaTime * self.speed
    local p = self._base
    self.transform.position = Vector3.new(p.x, p.y + math.sin(self._t) * self.height, p.z)
end

return Bobber
