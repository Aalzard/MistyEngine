-- LaserPointer: casts a ray forward (ignoring the Player layer), stretches a beam to the hit
-- and puts a dot there. Left click or E gives the Rigidbody it points at a shove.
local LaserPointer = {
    range = 25,
    pushForce = 6,
    height = 0.4,
    beam = "Laser Beam",
    dot = "Laser Dot",
}

function LaserPointer:Start()
    self._beam = GameObject.Find(self.beam).transform
    self._dot = GameObject.Find(self.dot)
    self._mask = Physics.DefaultRaycastLayers & ~LayerMask.GetMask("Player")
end

function LaserPointer:LateUpdate()
    local forward = self.transform.forward
    local origin = self.transform.position + Vector3.up * self.height + forward * 0.5
    local hit = Physics.Raycast(origin, forward, self.range, self._mask)
    local length = self.range
    if hit ~= nil then length = hit.distance end
    self._beam.position = origin + forward * (length / 2)
    self._beam.eulerAngles = self.transform.eulerAngles
    self._beam.localScale = Vector3.new(0.03, 0.03, length)
    self._dot:SetActive(hit ~= nil)
    if hit == nil then return end
    self._dot.transform.position = hit.point
    if (Input.GetMouseButtonDown(0) or Input.GetKeyDown("e")) and hit.rigidbody ~= nil then
        hit.rigidbody:AddForce(forward * self.pushForce, ForceMode.Impulse)
    end
end

return LaserPointer
