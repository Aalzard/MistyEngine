-- SplineRider: reads the spline the car follows and pauses / resumes it with P.
local SplineRider = {
    road = "Winding Road",
}

function SplineRider:Start()
    self.anim = self.gameObject:GetComponent("SplineAnimate")
    local road = GameObject.Find(self.road):GetComponent("SplineContainer")
    local start = road:EvaluatePosition(0)
    Debug.Log(string.format("Road: %.1f m, %d knots, starts at (%.1f, %.1f)",
        road:CalculateLength(), road:KnotCount(), start.x, start.z))
end

function SplineRider:Update()
    if Input.GetKeyDown("p") then
        if self.anim.isPlaying then self.anim:Pause() else self.anim:Play() end
    end
end

return SplineRider
