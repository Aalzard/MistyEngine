-- NetCoin: the server gives a point to the player who touches it and moves it elsewhere;
-- every client hears about it with a client RPC (the coin pops).
local NetCoin = {
    radius = 1.0,   -- pickup distance in metres
    arena = 8,
    spin = 120,     -- degrees per second (only for looks: not replicated)
}

function NetCoin:Update()
    self.transform:Rotate(0, self.spin * Time.deltaTime, 0)
    if self._pop and self._pop > 0 then
        self._pop = self._pop - Time.deltaTime
        local s = 0.6 + math.max(self._pop, 0) * 1.5
        self.transform.localScale = Vector3.new(s, s, s)
    end
    if not self.IsServer then return end
    for _, id in ipairs(Network.ConnectedClientsIds) do
        local go = Network.GetPlayerObject(id)
        if go and (go.transform.position - self.transform.position).magnitude < self.radius then
            local player = go:GetComponent("NetPlayer")
            player.score = player.score + 1
            self:CollectedClientRpc(player.playerName)
            local a = self.arena
            self.transform.position = Vector3.new(math.random() * 2 * a - a, 0.6, math.random() * 2 * a - a)
            return
        end
    end
end

function NetCoin:CollectedClientRpc(name)
    Debug.Log(name .. " picked up the coin")
    self._pop = 0.25
end

return NetCoin
