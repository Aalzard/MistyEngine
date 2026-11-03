-- AnimatorControl: drives the object's Animator from the keyboard and shows the current state.
-- Space = Jump (trigger), 1 / 2 / 3 = Speed 0 / 0.5 / 1 (blend tree), 4 = Dance on / off (bool).
local AnimatorControl = {
    stateText = "State Text",   -- UI Text that shows the current state
    smoothing = 5,              -- how fast Speed eases towards the chosen value
}

local STATES = { "Locomotion", "Jump", "Dance" }

function AnimatorControl:Start()
    self._anim = self.gameObject:GetComponent("Animator")
    local label = GameObject.Find(self.stateText)
    self._label = label and label:GetComponent("Text")
    self._speed = 0
end

function AnimatorControl:Update()
    local a = self._anim
    if not a then return end
    if Input.GetKeyDown("space") then a:SetTrigger("Jump") end
    if Input.GetKeyDown("1") then self._speed = 0 end
    if Input.GetKeyDown("2") then self._speed = 0.5 end
    if Input.GetKeyDown("3") then self._speed = 1 end
    if Input.GetKeyDown("4") then a:SetBool("Dance", not a:GetBool("Dance")) end

    local s = a:GetFloat("Speed")
    a:SetFloat("Speed", s + (self._speed - s) * math.min(1, Time.deltaTime * self.smoothing))

    if self._label then
        local info = a:GetCurrentAnimatorStateInfo(0)
        local name = "?"
        for _, n in ipairs(STATES) do
            if info:IsName(n) then name = n end
        end
        local fading = a:IsInTransition(0) and "  (blending)" or ""
        self._label.text = string.format("State: %s  %.0f%%   Speed %.2f%s",
            name, (info.normalizedTime % 1) * 100, a:GetFloat("Speed"), fading)
    end
end

return AnimatorControl
