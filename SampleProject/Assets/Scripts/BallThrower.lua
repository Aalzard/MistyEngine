-- BallThrower: left click throws a ball where the mouse points.
-- Hit the dummy to knock it over (R gets it back up), or the blocks on the ramp.
local BallThrower = {
    speed = 18,
    template = "Ball Template",
}

function BallThrower:Start()
    self._template = GameObject.Find(self.template)
end

function BallThrower:Update()
    if not Input.GetMouseButtonDown(0) or self._template == nil then return end
    local ray = Camera.main:ScreenPointToRay(Input.mousePosition)
    if ray == nil then return end
    local ball = Instantiate(self._template, ray.origin + ray.direction * 1.5)
    local body = ball:GetComponent("Rigidbody")
    body.isKinematic = false
    body.useGravity = true
    body.velocity = ray.direction * self.speed
end

return BallThrower
