-- CameraDirector: every few seconds gives the next shot the highest priority, so the Camera
-- Brain blends to it, and shows the live camera (and blend progress) in a label.
local CameraDirector = {
    shots = "Follow Cam,Wide Shot Cam,Follow Cam,Close-Up Cam",   -- virtual cameras, in order
    interval = 4,           -- seconds per shot
    livePriority = 20,      -- priority of the chosen shot
    label = "Camera Label", -- UI Text showing the live camera
}

function CameraDirector:Start()
    self._shots = {}
    self._base = {}
    for name in string.gmatch(self.shots, "[^,]+") do
        local go = GameObject.Find(name)
        local cam = go and go:GetComponent("VirtualCamera")
        if cam then
            table.insert(self._shots, cam)
            self._base[name] = self._base[name] or cam.priority
        else
            Debug.LogWarning("CameraDirector: no Virtual Camera named " .. name)
        end
    end
    local main = GameObject.Find("Main Camera")
    self._brain = main and main:GetComponent("CameraBrain")
    local label = GameObject.Find(self.label)
    self._label = label and label:GetComponent("Text")
    self._index = 1
    self._next = self.interval
end

function CameraDirector:Update()
    if #self._shots == 0 then return end
    if Time.time >= self._next then
        self._next = self._next + self.interval
        self._index = self._index % #self._shots + 1
        local chosen = self._shots[self._index]
        for _, cam in ipairs(self._shots) do
            cam.priority = self._base[cam.gameObject.name]
        end
        chosen.priority = self.livePriority
    end
    local b = self._brain
    if self._label and b then
        local live = b.activeVirtualCamera
        local text = "Live: " .. (live and live.name or "none")
        if b.isBlending then
            text = text .. string.format("   (blending %d%%)", math.floor(b.blendWeight * 100))
        end
        self._label.text = text
    end
end

return CameraDirector
