-- Crowd: walks its NavMeshAgent from where it starts to a goal and back, again and again.
local Crowd = {
    goal = Vector3.new(0, 0, 0),
    wait = 1,
}

function Crowd:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    self.home = self.transform.position
    self.toGoal = true
    self.timer = 0
    self.agent:SetDestination(self.goal)
end

function Crowd:Update()
    if self.agent.pathPending or self.agent.remainingDistance > 0.5 then
        return
    end
    self.timer = self.timer + Time.deltaTime
    if self.timer < self.wait then
        return
    end
    self.timer = 0
    self.toGoal = not self.toGoal
    if self.toGoal then
        self.agent:SetDestination(self.goal)
    else
        self.agent:SetDestination(self.home)
    end
end

return Crowd
