-- VerticalBobber: floats the object up and down above where it starts.
local VerticalBobber = {
    height = 2,                      -- metres above the start at the top
    speed = 0.8,                     -- radians per second
}

function VerticalBobber:Start()
    self.base = self.transform.position
end

function VerticalBobber:Update()
    local lift = (1 - Mathf.Cos(Time.time * self.speed)) * 0.5 * self.height
    self.transform.position = Vector3.new(self.base.x, self.base.y + lift, self.base.z)
end

return VerticalBobber
