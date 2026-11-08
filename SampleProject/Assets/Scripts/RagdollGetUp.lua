-- RagdollGetUp: R blends the ragdoll back to its animation; K knocks it over.
local RagdollGetUp = {
    blendTime = 0.6,
}

function RagdollGetUp:Update()
    if Input.GetKeyDown("r") then
        Ragdoll.Disable(self.gameObject, self.blendTime)
    end
    if Input.GetKeyDown("k") then
        Ragdoll.Enable(self.gameObject)
    end
end

return RagdollGetUp
