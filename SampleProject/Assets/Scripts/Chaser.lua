-- Chaser: keeps sending its NavMeshAgent to the target object (re-planned a few times a second).
local Chaser = {
    target = "Click Marker",
    repath = 0.25,
}

function Chaser:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    self.goal = GameObject.Find(self.target)
    self.timer = 0
end

function Chaser:Update()
    self.timer = self.timer - Time.deltaTime
    if self.goal ~= nil and self.timer <= 0 then
        self.timer = self.repath
        self.agent:SetDestination(self.goal.transform.position)
    end
end

return Chaser
