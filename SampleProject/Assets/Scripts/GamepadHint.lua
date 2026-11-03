-- GamepadHint: shows the controls in this object's Text, naming the first connected gamepad.
local GamepadHint = {}

function GamepadHint:Start()
    self.label = self.gameObject:GetComponent("Text")
end

function GamepadHint:Update()
    local pad = nil
    for _, name in ipairs(Input.GetJoystickNames()) do
        if name ~= "" then
            pad = name
            break
        end
    end
    local line = "No gamepad: plug one in to move with the left stick"
    if pad ~= nil then
        line = "Gamepad: " .. pad .. " (left stick moves, Y jumps)"
    end
    local text = "Move: WASD / arrow keys   Jump: Space\n" .. line
        .. "\nThe camera's Culling Mask hides the Hidden layer"
    if text ~= self._shown then
        self.label.text = text
        self._shown = text
    end
end

return GamepadHint
