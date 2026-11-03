-- Patrol: walks a NavMeshAgent from waypoint to waypoint (comma-separated object names),
-- waiting a moment at each one.
local Patrol = {
    waypoints = "Waypoint 1, Waypoint 2",
    wait = 0.5,
}

function Patrol:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    self.points = {}
    for name in string.gmatch(self.waypoints, "[^,]+") do
        local go = GameObject.Find(name:match("^%s*(.-)%s*$"))
        if go ~= nil then
            table.insert(self.points, go.transform.position)
        end
    end
    self.index = 1
    self.timer = 0
    self:GoNext()
end

function Patrol:GoNext()
    if #self.points == 0 then
        return
    end
    self.agent:SetDestination(self.points[self.index])
    self.index = self.index % #self.points + 1
end

function Patrol:Update()
    if not self.agent.pathPending and self.agent.remainingDistance < 0.3 then
        self.timer = self.timer + Time.deltaTime
        if self.timer >= self.wait then
            self.timer = 0
            self:GoNext()
        end
    end
end

return Patrol
