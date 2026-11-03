-- TerrainHover: circles around its start point, keeping a fixed height above the terrain
-- (Terrain.activeTerrain:SampleHeight).
local TerrainHover = {
    radius = 20,
    speed = 0.35,
    height = 1.5,
}

function TerrainHover:Start()
    self._terrain = Terrain.activeTerrain
    self._center = self.transform.position
    self._angle = 0
end

function TerrainHover:Update()
    self._angle = self._angle + self.speed * Time.deltaTime
    local offset = Vector3.new(math.cos(self._angle), 0, math.sin(self._angle)) * self.radius
    local p = self._center + offset
    local ground = self._terrain:SampleHeight(p) + self._terrain:GetPosition().y
    self.transform.position = Vector3.new(p.x, ground + self.height, p.z)
end

return TerrainHover
