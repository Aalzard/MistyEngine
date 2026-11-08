-- CharacterControl: drives the character's Animator.
-- W / S walk forwards and back, A / D step sideways, Shift runs (a 2D blend tree; the clips'
-- root motion moves the character), Left / Right turn, Q waves (fades the Upper Body layer in
-- and out), 1 bows and 2 cheers (the Emotes sub-state machine).
local CharacterControl = {
    turnSpeed = 120, -- degrees per second
    damp = 0.15, -- seconds MoveX / MoveZ take to follow the keys
    stateText = "State Text",
}

function CharacterControl:Start()
    self._anim = self.gameObject:GetComponent("Animator")
    self._wave = 0
    self._waving = false
    local label = GameObject.Find(self.stateText)
    self._label = label and label:GetComponent("Text")
end

function CharacterControl:Update()
    local a = self._anim
    if not a then return end
    local dt = Time.deltaTime
    local x, z = 0, 0
    if Input.GetKey("w") then z = z + 1 end
    if Input.GetKey("s") then z = z - 1 end
    if Input.GetKey("d") then x = x + 1 end
    if Input.GetKey("a") then x = x - 1 end
    if z > 0 and Input.GetKey("left shift") then z = 2 end
    a:SetFloat("MoveX", x, self.damp, dt)
    a:SetFloat("MoveZ", z, self.damp, dt)

    local turn = 0
    if Input.GetKey("left") then turn = turn - 1 end
    if Input.GetKey("right") then turn = turn + 1 end
    if turn ~= 0 then self.transform:Rotate(0, turn * self.turnSpeed * dt, 0) end

    if Input.GetKeyDown("q") then self._waving = not self._waving end
    local target = self._waving and 1 or 0
    self._wave = self._wave + (target - self._wave) * math.min(1, dt * 4)
    a:SetLayerWeight(a:GetLayerIndex("Upper Body"), self._wave)

    if Input.GetKeyDown("1") then a:SetInteger("EmoteId", 0); a:SetTrigger("Emote") end
    if Input.GetKeyDown("2") then a:SetInteger("EmoteId", 1); a:SetTrigger("Emote") end

    if self._label then
        local info = a:GetCurrentAnimatorStateInfo(0)
        self._label.text = string.format("%s   Move (%.2f, %.2f)   Wave %.0f%%",
            info.fullPath, a:GetFloat("MoveX"), a:GetFloat("MoveZ"), self._wave * 100)
    end
end

return CharacterControl
