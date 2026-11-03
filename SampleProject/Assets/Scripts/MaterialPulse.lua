-- Pulses this object's own material colour and alpha.
-- `renderer.material` is a copy for this object only; other objects using Ghost.mat keep theirs.
local MaterialPulse = { speed = 2, minAlpha = 0.1, maxAlpha = 0.6 }

function MaterialPulse:Start()
    self.renderer = self.gameObject:GetComponent("MeshRenderer")
    self.base = self.renderer.sharedMaterial.color
end

function MaterialPulse:Update()
    local t = (math.sin(Time.time * self.speed) + 1) * 0.5
    local c = self.base
    local a = self.minAlpha + (self.maxAlpha - self.minAlpha) * t
    self.renderer.material.color = Color.new(c.r, c.g + 0.3 * t, c.b, a)
end

return MaterialPulse
