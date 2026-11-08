-- PlayerActions: Player Input messages in the InputSystemDemo (Send Messages behaviour).
-- OnInteract runs when Interact performs (hold E, or X on a gamepad, for half a second): the
-- gamepad rumbles briefly. OnControlsChanged notes the control scheme in use. The hint text
-- shows the scheme and the current Jump binding. Moving, looking, jumping, sprinting and
-- crouching are read by FirstPersonController from the same Player Input.
local PlayerActions = {
    rumbleTime = 0.3, -- seconds
}

function PlayerActions:Start()
    self.player = self.gameObject:GetComponent("PlayerInput")
    self.jump = self.player.actions:FindAction("Player/Jump")
    local hint = GameObject.Find("Hint Text")
    self.hint = hint and hint:GetComponent("Text")
    self._rumble = 0
    self._interacts = 0
    self._changes = 0
end

function PlayerActions:OnInteract()
    self._interacts = self._interacts + 1
    local pad = self.player:GetDevice("Gamepad")
    if pad ~= nil then
        pad:SetMotorSpeeds(0.4, 0.9)
        self._rumble = self.rumbleTime
    end
end

function PlayerActions:OnControlsChanged(player)
    self._changes = self._changes + 1
end

function PlayerActions:Update()
    if self._rumble > 0 then
        self._rumble = self._rumble - Time.unscaledDeltaTime
        if self._rumble <= 0 then
            local pad = self.player:GetDevice("Gamepad")
            if pad ~= nil then pad:ResetHaptics() end
        end
    end
    if self.hint == nil then return end
    local text = "Move  Look  Jump: " .. self.jump:GetBindingDisplayString()
        .. "  Sprint  Crouch    Hold E / X: interact    Tab / Start: rebind menu\n"
        .. "Control scheme: " .. tostring(self.player.currentControlScheme)
        .. "    Interactions: " .. self._interacts
    if text ~= self._shown then
        self.hint.text = text
        self._shown = text
    end
end

return PlayerActions
