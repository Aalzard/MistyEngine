-- AnimationControl: Space pauses/resumes the object's Animation, Up/Down change its speed.
local AnimationControl = {
    clip = "Bob",       -- clip played by Space
    step = 0.25,        -- speed change per key press
}

function AnimationControl:Start()
    self.anim = self.gameObject:GetComponent("Animation")
end

function AnimationControl:Update()
    if not self.anim then return end
    if Input.GetKeyDown("space") then
        if self.anim.isPlaying then
            self.anim:Stop()
        else
            self.anim:Play(self.clip)
        end
    end
    if Input.GetKeyDown("up") then
        self.anim.speed = self.anim.speed + self.step
        Debug.Log("Animation speed", self.anim.speed)
    elseif Input.GetKeyDown("down") then
        self.anim.speed = math.max(0, self.anim.speed - self.step)
        Debug.Log("Animation speed", self.anim.speed)
    end
end

return AnimationControl
