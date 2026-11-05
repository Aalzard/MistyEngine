-- SaveLocMenu: the SaveLocDemo panel. Save / Load / Delete one slot, push the crates, spawn
-- coins and switch language with the dropdown. Every label is a Localized Text, so this script
-- only changes keys and arguments; the texts follow the current locale by themselves.
local SaveLocMenu = {
    slot = "demo",   -- the save slot the buttons use
}

local LOCALES = { "en", "fr", "es", "ar" }

function SaveLocMenu:Start()
    self._status = GameObject.Find("Status"):GetComponent("LocalizedText")
    self._info = GameObject.Find("Slot Info"):GetComponent("LocalizedText")
    self._language = GameObject.Find("Language Dropdown"):GetComponent("Dropdown")
    -- The dropdown shows the locale this session started with (or kept after a Load).
    for i, code in ipairs(LOCALES) do
        if Localization.locale == code then self._language.value = i - 1 end
    end
    if self._loaded then self:Status("status.loaded") end
    self:ShowSlot()
end

function SaveLocMenu:Status(key)
    self._status.key = key
    self._status.args = { self.slot }
end

-- "Last save: <date>, <scene>, played <time>" for the slot, or "No save yet".
function SaveLocMenu:ShowSlot()
    for _, s in ipairs(SaveSystem.ListSlots()) do
        if s.slot == self.slot then
            self._info.key = "slot.info"
            self._info.args = { s.date, s.scene, s.playTimeText }
            return
        end
    end
    self._info.key = "slot.empty"
    self._info.args = {}
end

function SaveLocMenu:SaveGame()
    if SaveSystem.Save(self.slot) then self:Status("status.saved") end
    self:ShowSlot()
end

-- The scene reloads after this frame; OnLoad below runs in the reloaded scene.
function SaveLocMenu:LoadGame()
    if SaveSystem.Exists(self.slot) then
        SaveSystem.Load(self.slot)
    else
        self:Status("status.nosave")
    end
end

function SaveLocMenu:DeleteGame()
    if SaveSystem.Delete(self.slot) then
        self:Status("status.deleted")
    else
        self:Status("status.nosave")
    end
    self:ShowSlot()
end

function SaveLocMenu:PushCrates()
    for i = 1, 3 do
        local crate = GameObject.Find("Crate " .. i)
        if crate then
            local rb = crate:GetComponent("Rigidbody")
            rb.velocity = Vector3.new(math.random() * 4 - 2, 5, math.random() * 4 - 2)
        end
    end
end

function SaveLocMenu:SpawnCoin()
    Instantiate("Assets/Prefabs/SaveLocCoin.prefab",
        Vector3.new(math.random() * 6 - 3, 3, math.random() * 3))
end

-- Called by the Language Dropdown's On Value Changed with the option index.
function SaveLocMenu:LanguageChanged(index)
    local code = LOCALES[index + 1]
    if code then Localization.locale = code end
end

-- Remember which slot was loaded, to say so after the reload.
function SaveLocMenu:OnSave(data)
    data.slot = self.slot
end

function SaveLocMenu:OnLoad(data)
    self._loaded = true
    self.slot = data.slot or self.slot
end

return SaveLocMenu
