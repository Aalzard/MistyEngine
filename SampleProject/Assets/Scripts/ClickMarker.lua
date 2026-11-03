-- ClickMarker: left-click the Game view to move this marker onto the clicked spot of the
-- nav mesh (a ray from the camera through the mouse, then the nearest nav mesh point).
local ClickMarker = {
    hover = 0.25,
}

function ClickMarker:Update()
    if Input.GetMouseButtonDown(0) then
        local ray = Camera.main:ScreenPointToRay(Input.mousePosition)
        local hit = Physics.Raycast(ray.origin, ray.direction, 200)
        if hit ~= nil then
            local onMesh = NavMesh.SamplePosition(hit.point, 1.5)
            if onMesh ~= nil then
                self.transform.position = onMesh.position + Vector3.new(0, self.hover, 0)
            end
        end
    end
end

return ClickMarker
