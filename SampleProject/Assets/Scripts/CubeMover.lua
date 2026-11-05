-- CubeMover: moves the object with WASD or the arrow keys (its position is saved by its
-- Saveable component).
local CubeMover = {
    speed = 3,
}

function CubeMover:Update()
    local x, z = 0, 0
    if Input.GetKey("right") or Input.GetKey("d") then x = x + 1 end
    if Input.GetKey("left") or Input.GetKey("a") then x = x - 1 end
    if Input.GetKey("up") or Input.GetKey("w") then z = z + 1 end
    if Input.GetKey("down") or Input.GetKey("s") then z = z - 1 end
    if x ~= 0 or z ~= 0 then
        local p = self.transform.position
        local step = self.speed * Time.deltaTime
        self.transform.position = Vector3.new(p.x + x * step, p.y, p.z + z * step)
    end
end

return CubeMover
