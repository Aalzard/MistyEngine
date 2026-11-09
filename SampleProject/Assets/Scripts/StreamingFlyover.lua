-- StreamingDemo: the camera flies a loop over the 4 km world. Cells stream in around it
-- (the main camera is the World Partition's streaming source); unloaded cells show their HLOD.
local F = {
    radius = 1400,  -- metres from the world's centre
    height = 300,   -- altitude of the loop
    speed = 70,     -- metres per second
}

function F:Start()
    self.angle = 0
    self.ready = false
    self.transform.position = self:at(0)
    -- Wait until the cells around the start are in.
    WorldPartition.OnAreaLoaded(self:at(0), 400, function()
        self.ready = true
        Debug.Log("StreamingDemo: start area loaded (" .. WorldPartition.loadedCells .. " cells)")
    end)
end

function F:at(angle)
    return Vector3.new(math.cos(angle) * self.radius, self.height, math.sin(angle) * self.radius * 0.8)
end

function F:Update()
    if not self.ready then return end
    self.angle = self.angle + Time.deltaTime * self.speed / self.radius
    local here = self:at(self.angle)
    local ahead = self:at(self.angle + 0.08)
    self.transform.position = here
    self.transform:LookAt(Vector3.new(ahead.x, ahead.y - 110, ahead.z))
end

return F
