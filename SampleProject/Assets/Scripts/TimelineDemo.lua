-- TimelineDemo: logs the timeline's signals, and when the director stops.
local TimelineDemo = {}

function TimelineDemo:Start()
    local director = self.gameObject:GetComponent("PlayableDirector")
    director.stopped:AddListener(function()
        print("Timeline finished")
    end)
end

-- Called by the Signal track and the markers row (the marker's text is the parameter).
function TimelineDemo:OnCue(text)
    print("Signal: " .. text)
end

return TimelineDemo
