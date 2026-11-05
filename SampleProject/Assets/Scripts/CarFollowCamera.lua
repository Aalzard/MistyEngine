-- CarFollowCamera: stays behind and above the target, easing after it, and looks at it.
local CarFollowCamera = {
    target = "Car",
    distance = 7.5,
    height = 2.8,
    lookHeight = 0.8,
    -- How quickly the camera catches up (higher = stiffer).
    positionDamping = 4,
}

function CarFollowCamera:Start()
    self._target = GameObject.Find(self.target).transform
end

function CarFollowCamera:LateUpdate()
    local t = self._target
    -- Behind the car on the ground plane, so it doesn't swing when the car pitches.
    local back = Vector3.new(t.forward.x, 0, t.forward.z)
    if back.magnitude < 0.01 then back = Vector3.forward end
    back = back.normalized
    local wanted = t.position - back * self.distance + Vector3.up * self.height
    local k = Mathf.Clamp(Time.deltaTime * self.positionDamping, 0, 1)
    self.transform.position = Vector3.Lerp(self.transform.position, wanted, k)
    self.transform:LookAt(t.position + Vector3.up * self.lookHeight)
end

return CarFollowCamera
