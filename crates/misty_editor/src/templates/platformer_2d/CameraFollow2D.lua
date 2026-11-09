-- CameraFollow2D: eases the camera towards the target in X/Y, keeping its own depth.
-- The view stops at the level's edges (left, right, bottom in world units; set one to nil
-- for no limit on that side).
local CameraFollow2D = {
    target = "Player",
    smoothing = 6,
    height = 1,
    left = -14,
    right = 37,
    bottom = -4,
}

function CameraFollow2D:Start()
    self.followed = GameObject.Find(self.target)
    self.camera = self.gameObject:GetComponent("Camera")
end

-- The closest camera position to (x, y) that keeps the view inside the level.
function CameraFollow2D:Clamp(x, y)
    local cam = self.camera
    if cam == nil or not cam.orthographic then
        return x, y
    end
    local halfHeight = cam.orthographicSize
    local halfWidth = halfHeight * Screen.width / math.max(Screen.height, 1)
    if self.left ~= nil and self.right ~= nil and self.right - self.left < halfWidth * 2 then
        x = (self.left + self.right) / 2
    else
        if self.left ~= nil then
            x = math.max(x, self.left + halfWidth)
        end
        if self.right ~= nil then
            x = math.min(x, self.right - halfWidth)
        end
    end
    if self.bottom ~= nil then
        y = math.max(y, self.bottom + halfHeight)
    end
    return x, y
end

function CameraFollow2D:LateUpdate()
    if self.followed == nil then
        return
    end
    local p = self.transform.position
    local t = self.followed.transform.position
    local tx, ty = self:Clamp(t.x, t.y + self.height)
    local k = Mathf.Clamp(self.smoothing * Time.deltaTime, 0, 1)
    local x = p.x + (tx - p.x) * k
    local y = p.y + (ty - p.y) * k
    self.transform.position = Vector3.new(x, y, p.z)
end

return CameraFollow2D
