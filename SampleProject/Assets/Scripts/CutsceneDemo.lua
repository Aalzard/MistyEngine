-- CutsceneDemo: logs the cutscene's cues, shakes the cameras on the explosion signal, and
-- says when the director hands over to the Free Look camera.
local CutsceneDemo = {}

function CutsceneDemo:Start()
    local director = self.gameObject:GetComponent("PlayableDirector")
    director.stopped:AddListener(function()
        print("Cutscene over: Free Look camera")
    end)
end

-- Signal track markers (the marker's text is the parameter).
function CutsceneDemo:OnCue(text)
    print("Cue: " .. text)
end

-- The explosion signal: every virtual camera with an Impulse Listener shakes.
function CutsceneDemo:OnExplosion()
    print("Boom!")
    Impulse.GenerateImpulse(GameObject.Find("Barrel"))
end

return CutsceneDemo
