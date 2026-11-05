-- Pulse (Example Effects package): scales the object in and out.
local Pulse = { amount = 0.15, speed = 3 }

function Pulse:Start()
    self._t = 0
end

function Pulse:Update()
    self._t = self._t + Time.deltaTime * self.speed
    local s = 1 + math.sin(self._t) * self.amount
    self.transform.localScale = Vector3.new(s, s, s)
end

return Pulse
