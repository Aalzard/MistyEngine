-- PlayerMove: walk with WASD or the arrow keys.
local PlayerMove = {
    speed = 5,
}

function PlayerMove:Update()
    local h = Input.GetAxis("Horizontal")
    local v = Input.GetAxis("Vertical")
    if h ~= 0 or v ~= 0 then
        local step = Vector3.new(h, 0, v) * (self.speed * Time.deltaTime)
        self.transform.position = self.transform.position + step
    end
end

return PlayerMove
