-- Custom Inspector for Satellite (editor scripting sample): the usual fields, a summary and two
-- buttons, plus a handle in the Scene view that drags the orbit's centre.
local SatelliteInspector = CustomInspector("Satellite")

function SatelliteInspector:OnInspectorGUI()
    DrawDefaultInspector()
    local t = self.target
    if t.speed == 0 then
        GUI.HelpBox("Speed is 0: the object stands still.", "warning")
    else
        local lap = 360 / math.abs(t.speed)
        GUI.HelpBox(string.format("One lap takes %.1f s and covers %.1f m.", lap, 2 * math.pi * t.radius))
    end
    GUI.Horizontal(function()
        if GUI.Button("Centre below object") then
            local p = t.transform.position
            t.center = Vector3.new(p.x, t.center.y, p.z)
        end
        if GUI.Button("Centre on origin") then
            t.center = Vector3.zero
        end
    end)
end

function SatelliteInspector:OnSceneGUI()
    self.target.center = Handles.Position(self.target.center)
    Handles.Label(self.target.center, "Orbit centre")
end
