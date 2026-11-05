-- ProbeSwing: swings the sphere between the red and the blue wall. Its Mesh Renderer blends
-- the baked light probes (Light Probes: Blend Probes), so it picks up the colour of the wall it
-- comes near. Every few seconds it logs the light the probes give at its position
-- (LightProbes.GetInterpolatedProbe).
local ProbeSwing = {
    range = 3.6,
    speed = 0.6,
    height = 1.1,
    logEvery = 3,
}

function ProbeSwing:Start()
    self._next = 0
end

function ProbeSwing:Update()
    local x = Mathf.Sin(Time.time * self.speed) * self.range
    self.transform.position = Vector3.new(x, self.height, 0)
    if Time.time >= self._next then
        self._next = Time.time + self.logEvery
        local probe = LightProbes.GetInterpolatedProbe(self.transform.position)
        local c = probe:Evaluate(Vector3.new(0, 1, 0))
        Debug.Log(string.format("Light from the probes at x = %.1f: %.2f %.2f %.2f", x, c.r, c.g, c.b))
    end
end

return ProbeSwing
