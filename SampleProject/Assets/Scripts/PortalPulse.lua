-- Pulses the Magic Portal through the VisualEffect API; Space stops / restarts it.
local PortalPulse = { speed = 1.5 }

function PortalPulse:Start()
    self.fx = self.gameObject:GetComponent("VisualEffect")
    self.t = 0
end

function PortalPulse:Update()
    self.t = self.t + Time.deltaTime * self.speed
    local k = 0.5 + 0.5 * math.sin(self.t)
    self.fx:SetFloat("Rate", 400 + 600 * k)
    self.fx:SetColor("Color", Color.new(1.2 + k, 0.4, 3.5 - k, 1))
    if Input.GetKeyDown("space") then
        if self.fx.isPlaying then
            self.fx:Stop()
        else
            self.fx:Play()
        end
        Debug.Log("Portal particles alive:", self.fx.aliveCount)
    end
end

return PortalPulse
