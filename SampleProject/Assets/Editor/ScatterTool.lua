-- Scatter Tool (editor scripting sample): Tools > Scatter Tool opens a window that places copies
-- of a prefab around the selected object (or the origin) as one undo step. Tools > Align to
-- Floor shows a menu item with a validate function and a shortcut.
local Scatter = ToolWindow("Scatter Tool")
Scatter.count = 10
Scatter.radius = 5
Scatter.randomRotation = true
Scatter.scaleJitter = 0.2

function Scatter:OnGUI()
    -- A prefab selected in the Project window is picked up.
    local asset = Selection.asset
    if not self.prefab and asset and asset:match("%.prefab$") then
        self.prefab = asset
    end
    self.prefab = GUI.ObjectField("Prefab", self.prefab, "prefab")
    self.count = math.max(1, GUI.IntField("Count", self.count))
    self.radius = GUI.Slider("Radius", self.radius, 0.5, 50)
    self.randomRotation = GUI.Toggle("Random rotation", self.randomRotation)
    self.scaleJitter = GUI.Slider("Scale jitter", self.scaleJitter, 0, 1)
    GUI.Space(6)
    local centre = Selection.active
    GUI.Label("Around: " .. (centre and centre.name or "the origin"))
    if not self.prefab then
        GUI.HelpBox("Choose a prefab, or select one in the Project window.")
    elseif GUI.Button("Scatter " .. self.count .. " copies") then
        self:Scatter(centre)
    end
end

function Scatter:Scatter(centre)
    local origin = centre and centre.transform.position or Vector3.zero
    Undo.Record(nil, "Scatter Prefabs")
    local group = EditorScene.Create("Scattered")
    for _ = 1, self.count do
        local a = math.random() * math.pi * 2
        local r = math.sqrt(math.random()) * self.radius
        local copy = EditorScene.InstantiatePrefab(self.prefab, origin + Vector3.new(math.cos(a) * r, 0, math.sin(a) * r), group)
        if self.randomRotation then
            copy.transform.eulerAngles = Vector3.new(0, math.random() * 360, 0)
        end
        copy.transform.localScale = copy.transform.localScale * (1 + (math.random() * 2 - 1) * self.scaleJitter)
    end
    Selection.active = group
end

MenuItem("Tools/Scatter Tool", function()
    Scatter:Open()
end)

MenuItem("Tools/Align to Floor", function()
    Undo.Record(nil, "Align to Floor")
    for _, go in ipairs(Selection.objects) do
        local p = go.transform.position
        go.transform.position = Vector3.new(p.x, 0, p.z)
    end
end, {
    validate = function()
        return Selection.count > 0
    end,
    shortcut = "Ctrl+Alt+G",
})
