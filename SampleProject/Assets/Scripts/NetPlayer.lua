-- NetPlayer: one player's capsule. The owner moves it at once (client-side prediction) and
-- sends the move to the server, which moves the real one; the Network Transform corrects the
-- owner if the server disagrees. The name is written by the owner, score and colour by the server.
local NetPlayer = {
    speed = 5,     -- metres per second
    arena = 9,     -- half the floor's size
}
NetPlayer._attributes = {
    speed = Tooltip("Metres per second"),
    playerName = NetworkVariable({ write = "Owner" }),
    score = NetworkVariable(),
    colorIndex = NetworkVariable({ onChanged = "OnColorChanged" }),
}
NetPlayer.playerName = ""
NetPlayer.score = 0
NetPlayer.colorIndex = 0

local COLORS = {
    { 0.95, 0.35, 0.3 },
    { 0.3, 0.6, 1.0 },
    { 0.4, 0.85, 0.4 },
    { 1.0, 0.8, 0.25 },
}

function NetPlayer:OnNetworkSpawn()
    if self.IsServer then
        local slot = self.OwnerClientId % 4
        self.colorIndex = slot + 1
        self.transform.position = Vector3.new(-3 + slot * 2, 1, -3)
    end
    if self.IsOwner then
        self.playerName = "Player " .. (self.OwnerClientId + 1)
    end
    -- Only your own capsule wears the marker.
    self.transform:Find("You").gameObject:SetActive(self.IsOwner)
    self:OnColorChanged(0, self.colorIndex)
end

function NetPlayer:OnColorChanged(_, index)
    local c = COLORS[index]
    if c then
        self.gameObject:GetComponent("MeshRenderer").material.color = Color.new(c[1], c[2], c[3])
    end
end

function NetPlayer:Update()
    if not self.IsOwner then return end
    local h, v = Input.GetAxis("Horizontal"), Input.GetAxis("Vertical")
    if h == 0 and v == 0 then return end
    local delta = Vector3.new(h, 0, v) * (self.speed * Time.deltaTime)
    self:Move(delta)
    if not self.IsServer then self:MoveServerRpc(delta) end
end

function NetPlayer:Move(delta)
    local p = self.transform.position + delta
    p.x = math.max(-self.arena, math.min(self.arena, p.x))
    p.z = math.max(-self.arena, math.min(self.arena, p.z))
    self.transform.position = p
end

-- Runs on the server for the owner's input; too-long moves are ignored.
function NetPlayer:MoveServerRpc(delta)
    if delta.magnitude > self.speed * 0.1 then return end
    self:Move(delta)
end

return NetPlayer
