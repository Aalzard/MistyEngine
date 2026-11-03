-- SettingsMenu: the UIWidgetsDemo panel. Listens to the slider, toggle, input field, dropdown and
-- the level buttons in the scroll list, and shows the current choices in the Summary text.
local SettingsMenu = {
    summary = "Summary",     -- name of the object with the summary Text
    levels = 8,              -- buttons "Level 1" … "Level N" in the scroll list
}

function SettingsMenu:Start()
    self._summary = GameObject.Find(self.summary):GetComponent("Text")
    self._volume = GameObject.Find("Volume Slider"):GetComponent("Slider")
    self._fullscreen = GameObject.Find("Fullscreen Toggle"):GetComponent("Toggle")
    self._name = GameObject.Find("Name Field"):GetComponent("InputField")
    self._quality = GameObject.Find("Quality Dropdown"):GetComponent("Dropdown")
    self._level = 1

    local refresh = function() self:Show() end
    self._volume.onValueChanged:AddListener(refresh)
    self._fullscreen.onValueChanged:AddListener(refresh)
    self._name.onValueChanged:AddListener(refresh)
    self._quality.onValueChanged:AddListener(refresh)
    for i = 1, self.levels do
        local b = GameObject.Find("Level " .. i)
        if b then
            b:GetComponent("Button").onClick:AddListener(function()
                self._level = i
                self:Show()
            end)
        end
    end
    self:Show()
end

-- Called by the Name Field's On End Edit (set in the Inspector).
function SettingsMenu:NameEntered(text)
    if text == "" then
        Debug.Log("No name entered")
    else
        Debug.Log("Hello, " .. text .. "!")
    end
end

function SettingsMenu:Show()
    local name = self._name.text
    if name == "" then name = "(none)" end
    local options = self._quality.options
    self._summary.text = string.format(
        "Volume %d  |  Fullscreen %s  |  Level %d\nName %s  |  Quality %s",
        self._volume.value,
        self._fullscreen.isOn and "on" or "off",
        self._level,
        name,
        options[self._quality.value + 1] or "?"
    )
end

return SettingsMenu
