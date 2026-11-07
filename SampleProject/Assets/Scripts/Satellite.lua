-- Satellite: circles around a centre point. The editor scripting sample adds its Scene view
-- gizmos (below) and a custom Inspector with a centre handle (Assets/Editor/SatelliteInspector.lua).
local Satellite = {
    center = Vector3.new(0, 1, 0),
    radius = 3,
    speed = 45,
    height = 0,
    showPath = true,
    angle = 0,
}

-- How the Inspector shows the fields.
Satellite._attributes = {
    center = { Header("Orbit"), Tooltip("World point the object circles around") },
    radius = { Range(0.5, 20), Tooltip("Distance from the centre in metres") },
    speed = { Range(-360, 360), Tooltip("Degrees per second (negative: clockwise)") },
    height = { Tooltip("Height above the centre") },
    showPath = { Space(6), Header("Scene View"), Tooltip("Draw the orbit when selected") },
    angle = { HideInInspector() },
}

function Satellite:Start()
    local offset = self.transform.position - self.center
    self.angle = math.deg(math.atan(offset.z, offset.x))
end

function Satellite:Update()
    self.angle = self.angle + self.speed * Time.deltaTime
    local a = math.rad(self.angle)
    local offset = Vector3.new(math.cos(a) * self.radius, self.height, math.sin(a) * self.radius)
    self.transform.position = self.center + offset
end

-- Editor only: the centre, on every Satellite.
function Satellite:OnDrawGizmos()
    Gizmos.color = Color.new(1, 0.8, 0.25, 0.7)
    Gizmos.WireSphere(self.center, 0.15)
end

-- Editor only: the orbit, while the object is selected.
function Satellite:OnDrawGizmosSelected()
    if not self.showPath then
        return
    end
    Gizmos.color = Color.new(1, 0.8, 0.25)
    local steps, prev = 48, nil
    for i = 0, steps do
        local a = i / steps * math.pi * 2
        local p = self.center + Vector3.new(math.cos(a) * self.radius, self.height, math.sin(a) * self.radius)
        if prev then
            Gizmos.Line(prev, p)
        end
        prev = p
    end
    Gizmos.Line(self.center, self.transform.position)
end

return Satellite
