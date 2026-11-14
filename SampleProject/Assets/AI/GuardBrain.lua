-- GuardBrain: what the guard's Behaviour Tree calls with Run Lua nodes.
local GuardBrain = {
    damage = 10,
}

function GuardBrain:Awake()
    self.attacks = 0
end

-- Run Lua "Attack": one swing (the tree's Cooldown spaces them out).
function GuardBrain:Attack()
    self.attacks = self.attacks + 1
    Debug.Log(self.gameObject.name .. " attacks the player for " .. self.damage .. " damage")
    return true
end

return GuardBrain
