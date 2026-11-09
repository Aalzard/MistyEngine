-- MoonCycle: runs the moon through its phases (new, waxing crescent, first quarter,
-- gibbous, full, waning) so you can watch the terminator, the earthshine on the dark side
-- and the moonlight dimming and brightening with it.
local MoonCycle = {
    period = 40,      -- seconds for a whole lunar month
    start = 0.12,     -- phase at Start (0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter)
}

function MoonCycle:Start()
    RenderSettings.moonPhase = self.start
end

function MoonCycle:Update()
    local phase = self.start + Time.time / self.period
    RenderSettings.moonPhase = phase - Mathf.Floor(phase)
end

return MoonCycle
