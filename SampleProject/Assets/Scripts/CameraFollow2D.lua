-- CameraFollow2D: eases the camera towards the target in X/Y, keeping its own depth.
local CameraFollow2D = {
    target = "Player",
    smoothing = 6,
    height = 1,
}

function CameraFollow2D:Start()
    self.followed = GameObject.Find(self.target)
end

function CameraFollow2D:LateUpdate()
    if self.followed == nil then
        return
    end
    local p = self.transform.position
    local t = self.followed.transform.position
    local k = Mathf.Clamp(self.smoothing * Time.deltaTime, 0, 1)
    local x = p.x + (t.x - p.x) * k
    local y = p.y + (t.y + self.height - p.y) * k
    self.transform.position = Vector3.new(x, y, p.z)
end

return CameraFollow2D
