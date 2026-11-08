-- RagdollBall: a ball that hits a ragdoll's bone switches the ragdoll to physics
-- and passes on a shove.
local RagdollBall = {
    shove = 6,
}

function RagdollBall:Start()
    self.body = self.gameObject:GetComponent("Rigidbody")
end

function RagdollBall:OnCollisionEnter(other)
    if self.body.isKinematic then return end
    if Ragdoll.Enable(other) then
        local bone = other:GetComponent("Rigidbody")
        local v = self.body.velocity
        if bone ~= nil then
            bone.velocity = bone.velocity + v.normalized * self.shove
        end
        Debug.Log("Knocked over by a ball")
    end
end

return RagdollBall
