-- @behaviour-leaf Next Waypoint
-- @param waypoints Waypoint 1, Waypoint 2, Waypoint 3, Waypoint 4
-- @param key PatrolPoint
-- A custom Behaviour Tree leaf: picks the next waypoint of a comma-separated list of object
-- names (the place it reached is kept in the blackboard's PatrolIndex key) and writes its
-- position to a Vector key, ready for a Move To node.
local NextWaypoint = {}

function NextWaypoint:OnTick(dt)
    local names = {}
    for name in string.gmatch(tostring(self.params.waypoints or ""), "[^,]+") do
        names[#names + 1] = name:match("^%s*(.-)%s*$")
    end
    if #names == 0 then
        return false
    end
    local me = self.gameObject
    local index = (BehaviourTree.GetValue(me, "PatrolIndex") or 0) % #names + 1
    BehaviourTree.SetValue(me, "PatrolIndex", index)
    local waypoint = GameObject.Find(names[index])
    if waypoint == nil then
        return false
    end
    BehaviourTree.SetValue(me, self.params.key or "PatrolPoint", waypoint.transform.position)
    return true
end

return NextWaypoint
