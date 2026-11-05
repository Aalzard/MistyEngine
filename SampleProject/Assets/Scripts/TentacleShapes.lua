-- TentacleShapes: drives the Skinned Mesh Renderer's blend shapes from code.
-- Bulge and Pinch pulse out of step (weights are 0-100, like the Inspector sliders).
local TentacleShapes = {
    speed = 1.6,
}

function TentacleShapes:Start()
    self.renderer = self.gameObject:GetComponent("SkinnedMeshRenderer")
    self.bulge = self.renderer:GetBlendShapeIndex("Bulge")
    self.pinch = self.renderer:GetBlendShapeIndex("Pinch")
end

function TentacleShapes:Update()
    local t = Time.time * self.speed
    self.renderer:SetBlendShapeWeight(self.bulge, 50 + 50 * math.sin(t))
    self.renderer:SetBlendShapeWeight(self.pinch, 50 + 50 * math.sin(t * 0.7 + 2.0))
end

return TentacleShapes
