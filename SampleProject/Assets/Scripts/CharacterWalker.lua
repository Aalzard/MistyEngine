-- CharacterWalker: W/S walk, A/D turn, Space jumps. Needs a Character Controller.
-- Rigidbodies it walks into are pushed (OnControllerColliderHit).
local CharacterWalker = {
    speed = 4,
    turnSpeed = 120,
    jumpSpeed = 6,
    gravity = -15,
    pushPower = 2,
}

function CharacterWalker:Start()
    self.cc = self.gameObject:GetComponent("CharacterController")
    self._vy = 0
end

function CharacterWalker:Update()
    local dt = Time.deltaTime
    self.transform:Rotate(0, Input.GetAxis("Horizontal") * self.turnSpeed * dt, 0)
    local move = self.transform.forward * (Input.GetAxis("Vertical") * self.speed)
    if self.cc.isGrounded then
        self._vy = -1
        if Input.GetKeyDown("space") then self._vy = self.jumpSpeed end
    end
    self._vy = self._vy + self.gravity * dt
    self.cc:Move((move + Vector3.new(0, self._vy, 0)) * dt)
end

function CharacterWalker:OnControllerColliderHit(hit)
    local body = hit.rigidbody
    if body == nil or hit.moveDirection.y < -0.3 then return end
    body.velocity = Vector3.new(hit.moveDirection.x, 0, hit.moveDirection.z) * self.pushPower
end

return CharacterWalker
