-- BridgePlank: reports when its joint breaks (Break Force on the joint).
local BridgePlank = {}

function BridgePlank:OnJointBreak(force)
    Debug.Log("Bridge plank snapped:", self.gameObject.name, math.floor(force), "N")
end

return BridgePlank
