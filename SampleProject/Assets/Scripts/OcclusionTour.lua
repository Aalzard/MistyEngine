-- OcclusionTour: drives the camera around the city's streets, looking ahead.
-- O toggles the camera's occlusion culling (compare the Stats).
local OcclusionTour = {
    speed = 7,          -- metres per second
    height = 1.8,
    lookAhead = 8,      -- metres ahead the camera looks
}

-- Corners of the loop, on street centre lines (x, z).
local points = {
    {-13, -65}, {-13, 13}, {39, 13}, {39, -39},
    {-39, -39}, {-39, 39}, {13, 39}, {13, -65},
}

function OcclusionTour:Start()
    self.lengths = {}
    self.total = 0
    for i = 1, #points do
        local a, b = points[i], points[i % #points + 1]
        local len = math.abs(b[1] - a[1]) + math.abs(b[2] - a[2])
        self.lengths[i] = len
        self.total = self.total + len
    end
    self.distance = 0
    self.camera = self.gameObject:GetComponent("Camera")
end

-- Point at distance `d` along the loop.
function OcclusionTour:At(d)
    d = d % self.total
    for i = 1, #points do
        local len = self.lengths[i]
        if d <= len then
            local a, b = points[i], points[i % #points + 1]
            local t = d / len
            return Vector3.new(a[1] + (b[1] - a[1]) * t, self.height, a[2] + (b[2] - a[2]) * t)
        end
        d = d - len
    end
    return Vector3.new(points[1][1], self.height, points[1][2])
end

function OcclusionTour:Update()
    self.distance = self.distance + self.speed * Time.deltaTime
    self.transform.position = self:At(self.distance)
    self.transform:LookAt(self:At(self.distance + self.lookAhead))
    if Input.GetKeyDown("o") and Camera.main then
        local cam = Camera.main
        cam.useOcclusionCulling = not cam.useOcclusionCulling
        Debug.Log("Occlusion culling: " .. tostring(cam.useOcclusionCulling))
    end
end

return OcclusionTour
