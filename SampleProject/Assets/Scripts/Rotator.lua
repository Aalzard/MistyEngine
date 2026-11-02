-- Rotator: spins the object around an axis. Attach it to any GameObject.
local Rotator = {
    speed = 90,                      -- degrees per second
    axis = Vector3.new(0, 1, 0),
}

function Rotator:Update()
    self.transform:Rotate(self.axis * (self.speed * Time.deltaTime))
end

return Rotator
