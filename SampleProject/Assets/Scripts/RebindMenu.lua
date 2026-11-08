-- RebindMenu: Tab (Start on a gamepad) opens a menu that rebinds the Player actions while
-- playing. Each row's button listens for the next key or gamepad button
-- (action:PerformInteractiveRebinding; Esc cancels) and rebinds the action's binding for the
-- control scheme in use. Overrides are saved to PlayerPrefs and loaded back at start; Reset
-- removes them. Opening the menu switches the Player Input to its UI map, so the player stops
-- and FirstPersonController leaves the cursor free.
local RebindMenu = {
    prefsKey = "InputSystemDemo.rebinds",
}

local ROWS = { "Jump", "Sprint", "Crouch", "Interact" }

-- Index of the first plain binding of `action` in control scheme `scheme`.
local function bindingIndex(action, scheme)
    for i, b in ipairs(action.bindings) do
        local inScheme = scheme == nil or string.find(";" .. b.groups .. ";", ";" .. scheme .. ";", 1, true)
        if not b.isComposite and not b.isPartOfComposite and inScheme then
            return i - 1
        end
    end
    return 0
end

local function component(t, path, name)
    return t:Find(path).gameObject:GetComponent(name)
end

function RebindMenu:Start()
    self.player = GameObject.Find("Player"):GetComponent("PlayerInput")
    self.actions = self.player.actions
    local saved = PlayerPrefs.GetString(self.prefsKey, "")
    if saved ~= "" then
        self.actions:LoadBindingOverridesFromJson(saved)
    end
    local panel = self.transform:Find("Rebind Panel")
    self.panel = panel.gameObject
    self.status = component(panel, "Status", "Text")
    self.rows = {}
    for _, name in ipairs(ROWS) do
        local row = {
            action = self.actions:FindAction("Player/" .. name),
            label = component(panel, name .. " Binding/Text", "Text"),
        }
        component(panel, name .. " Binding", "Button").onClick:AddListener(function()
            self:_rebind(row)
        end)
        self.rows[#self.rows + 1] = row
    end
    component(panel, "Reset", "Button").onClick:AddListener(function() self:_reset() end)
    component(panel, "Close", "Button").onClick:AddListener(function() self:Close() end)
    self.actions:FindAction("Player/Menu").performed:AddListener(function() self:Open() end)
    self.actions:FindAction("UI/Close").performed:AddListener(function()
        if self._op == nil and (self._settle or 0) <= 0 then self:Close() end
    end)
    self._settle = 0
end

function RebindMenu:Update()
    if self._settle > 0 then self._settle = self._settle - 1 end
end

function RebindMenu:Open()
    self.panel:SetActive(true)
    self.player:SwitchCurrentActionMap("UI")
    Cursor.lockState = CursorLockMode.None
    self.status.text = "Click a binding, then press a key or a gamepad button"
    self:_refresh()
end

function RebindMenu:Close()
    if self._op ~= nil then self._op:Cancel() end
    self.panel:SetActive(false)
    self.player:SwitchCurrentActionMap("Player")
end

function RebindMenu:_refresh()
    local scheme = self.player.currentControlScheme
    for _, row in ipairs(self.rows) do
        row.label.text = row.action:GetBindingDisplayString(bindingIndex(row.action, scheme))
    end
end

function RebindMenu:_rebind(row)
    if self._op ~= nil then return end
    row.label.text = "Press a key or button..."
    self.status.text = "Listening (Esc cancels)"
    local index = bindingIndex(row.action, self.player.currentControlScheme)
    self._op = row.action:PerformInteractiveRebinding(index)
        :WithControlsExcluding("<Mouse>")
        :WithCancelingThrough("<Keyboard>/escape")
        :OnComplete(function(op) self:_done(op, true) end)
        :OnCancel(function(op) self:_done(op, false) end)
        :Start()
end

function RebindMenu:_done(op, completed)
    op:Dispose()
    self._op = nil
    self._settle = 2
    if completed then
        PlayerPrefs.SetString(self.prefsKey, self.actions:SaveBindingOverridesAsJson())
        PlayerPrefs.Save()
        self.status.text = "Saved to PlayerPrefs"
    else
        self.status.text = "Canceled"
    end
    self:_refresh()
end

function RebindMenu:_reset()
    self.actions:RemoveAllBindingOverrides()
    PlayerPrefs.SetString(self.prefsKey, "")
    PlayerPrefs.Save()
    self.status.text = "Bindings reset"
    self:_refresh()
end

return RebindMenu
