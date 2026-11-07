-- NetHud: host, join or leave with keys, and the scoreboard.
local NetHud = { address = "127.0.0.1" }

function NetHud:Start()
    self._text = self.gameObject:GetComponent("Text")
end

function NetHud:Update()
    if not Network.IsListening then
        if Input.GetKeyDown("h") then Network.StartHost() end
        if Input.GetKeyDown("j") then Network.StartClient(self.address) end
    elseif Input.GetKeyDown("l") then
        Network.Shutdown()
    end
    self._text.text = self:Describe()
end

function NetHud:Describe()
    if not Network.IsListening then
        return "H  host a game      J  join " .. self.address
            .. "\nor tick players in Window > Multiplayer > Multiplayer Play Mode and press Play"
    end
    if not (Network.IsServer or Network.IsConnectedClient) then
        return "Joining " .. self.address .. "..."
    end
    local lines = { Network.Mode .. "  -  WASD move, touch the coin, L leave" }
    local players = {}
    for _, go in ipairs(GameObject.FindGameObjectsWithTag("Player")) do
        local p = go:GetComponent("NetPlayer")
        if p and p.playerName ~= "" then players[#players + 1] = p end
    end
    table.sort(players, function(a, b) return a.score > b.score end)
    for _, p in ipairs(players) do
        lines[#lines + 1] = p.playerName .. "   " .. p.score
    end
    return table.concat(lines, "\n")
end

return NetHud
