-- PlayerController: WASD / arrow keys roll a Rigidbody, Space jumps.
-- Needs a Rigidbody and a collider (a Sphere works well). Click the Game view to give it the keyboard.
local PlayerController = {
    moveForce = 12,
    jumpSpeed = 5,
}

function PlayerController:Start()
    self.body = self.gameObject:GetComponent("Rigidbody")
    self.grounded = false
    Debug.Log("PlayerController ready on", self.gameObject.name)
end

function PlayerController:FixedUpdate()
    if self.body == nil then return end
    local input = Vector3.new(Input.GetAxis("Horizontal"), 0, Input.GetAxis("Vertical"))
    self.body:AddForce(input * self.moveForce)
end

function PlayerController:Update()
    if self.body ~= nil and self.grounded and Input.GetKeyDown("space") then
        self.body:AddForce(Vector3.up * self.jumpSpeed, ForceMode.VelocityChange)
        self.grounded = false
    end
end

function PlayerController:OnCollisionEnter(other)
    self.grounded = true
end

return PlayerController
