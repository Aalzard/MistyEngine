-- UIShowcase: drives the UIShowcase scene. The HUD's radial health ring and stamina bar move
-- with time (Image.fillAmount), the pause menu's buttons report presses (mouse, arrows or a
-- gamepad through the Event System), and the World Space sign's button calls Waved().
local UIShowcase = {
    status = "Status", -- the menu's status Text
    speed = 0.8, -- how fast the health ring breathes
}

function UIShowcase:Start()
    self._status = GameObject.Find(self.status):GetComponent("Text")
    self._ring = GameObject.Find("Health Ring"):GetComponent("Image")
    self._hp = GameObject.Find("Health Text"):GetComponent("Text")
    self._stamina = GameObject.Find("Stamina Fill"):GetComponent("Image")
    for _, name in ipairs({ "Resume", "Options", "Credits", "Quit" }) do
        local b = GameObject.Find(name)
        if b then
            b:GetComponent("Button").onClick:AddListener(function()
                self._status.text = name .. " pressed"
            end)
        end
    end
end

function UIShowcase:Update()
    local t = Time.time
    local health = 0.55 + 0.4 * math.sin(t * self.speed)
    self._ring.fillAmount = health
    self._hp.text = string.format("%d", math.floor(health * 100 + 0.5))
    -- Stamina drains and refills every four seconds.
    self._stamina.fillAmount = 1 - (t % 4) / 4
end

-- The sign's Wave button (its On Click, set in the Inspector).
function UIShowcase:Waved()
    self._status.text = "Hello from the sign!"
end

return UIShowcase
