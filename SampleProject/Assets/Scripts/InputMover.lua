-- InputMover: moves a Character Controller with the Horizontal / Vertical axes (WASD, arrow
-- keys or a gamepad's left stick; GetAxis eases in and out) and jumps on the Jump button
-- (Space or joystick button 3). Axes are set up in Project Settings > Input Manager.
local InputMover = {
    speed = 5,
    jumpSpeed = 6,
    gravity = -15,
}

function InputMover:Start()
    self.cc = self.gameObject:GetComponent("CharacterController")
    self._vy = 0
end

function InputMover:Update()
    local dt = Time.deltaTime
    local move = Vector3.new(Input.GetAxis("Horizontal"), 0, Input.GetAxis("Vertical"))
    if move.magnitude > 1 then
        move = move.normalized
    end
    if self.cc.isGrounded then
        self._vy = -1
        if Input.GetButtonDown("Jump") then
            self._vy = self.jumpSpeed
        end
    end
    self._vy = self._vy + self.gravity * dt
    self.cc:Move((move * self.speed + Vector3.new(0, self._vy, 0)) * dt)
end

return InputMover
