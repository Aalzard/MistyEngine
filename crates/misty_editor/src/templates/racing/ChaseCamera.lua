-- ChaseCamera: follows the car from behind and above, swinging round smoothly in corners,
-- pulling back and widening the view with speed (and more while boosting).
local ChaseCamera = {
    target = "Car",
    distance = 7,
    height = 2.6,
    lookHeight = 1.0,
    -- How far ahead of the car the camera looks (metres per m/s of speed).
    lookAhead = 0.08,
    -- How quickly the camera catches up (higher = stiffer).
    positionDamping = 6,
    baseFov = 62,
    fastFov = 76,
    boostFov = 84,
    -- Speed (m/s) giving the full fast field of view.
    fastSpeed = 60,
}

function ChaseCamera:Start()
    local go = GameObject.Find(self.target)
    self._target = go.transform
    self._body = go:GetComponent("Rigidbody")
    self._car = go:GetComponent("RaceCar")
    self._camera = self.gameObject:GetComponent("Camera")
    self._fov = self.baseFov
end

function ChaseCamera:LateUpdate()
    local t = self._target
    local velocity = self._body.velocity
    local speed = velocity.magnitude
    -- Behind the car on the ground plane, so it doesn't swing when the car pitches or flips.
    local back = Vector3.new(t.forward.x, 0, t.forward.z)
    if back.magnitude < 0.01 then back = Vector3.forward end
    back = back.normalized
    local fast = Mathf.Clamp01(speed / self.fastSpeed)
    local distance = self.distance * (1 + 0.25 * fast)
    local wanted = t.position - back * distance + Vector3.up * self.height
    local k = Mathf.Clamp01(Time.deltaTime * self.positionDamping)
    self.transform.position = Vector3.Lerp(self.transform.position, wanted, k)
    local flat = Vector3.new(velocity.x, 0, velocity.z)
    self.transform:LookAt(t.position + Vector3.up * self.lookHeight + flat * self.lookAhead)

    if self._camera ~= nil then
        local fov = Mathf.Lerp(self.baseFov, self.fastFov, fast)
        if self._car ~= nil and self._car.boosting then
            fov = self.boostFov
        end
        self._fov = Mathf.Lerp(self._fov, fov, Mathf.Clamp01(Time.deltaTime * 3))
        self._camera.fieldOfView = self._fov
    end
end

return ChaseCamera
