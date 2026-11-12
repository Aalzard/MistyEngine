-- AmbienceZones: blends an open-ground ambience loop (a meadow) with a zone loop (a forest's
-- birds and leaves) by how far the listener is from the zones.
--
-- Put it on an object with these children:
--   - Open Source: an Audio Source playing the open loop (looping, 2D);
--   - Zone Source: an Audio Source playing the zone loop (looping, 3D, Linear rolloff: Min
--     Distance = where it is at full volume, Max Distance = where it is gone);
--   - zones: children whose names start with Zone Prefix, each a box footprint on the ground
--     (its position, rotation and X/Z scale in metres; the height is ignored).
-- Each frame the zone loop's object glides to the closest point of the nearest zone (to the
-- listener itself inside one), so it is heard from the forest's direction and fades by its
-- rolloff as you walk away; within Envelop Distance it turns 2D and surrounds you. The open
-- loop fades to Open In Zone of its volume as you go in.
local AmbienceZones = {
    openSource = "Open Loop", -- child with the open loop's Audio Source
    zoneSource = "Zone Loop", -- child with the zone loop's Audio Source
    zonePrefix = "Area", -- children named like this are the zones
    openVolume = 0.6,
    openInZone = 0.25, -- share of the open loop left inside a zone
    zoneVolume = 0.8,
    blendDistance = 50, -- metres outside a zone over which the open loop fades
    envelopDistance = 20, -- metres from a zone where its loop starts to surround you
    glideTime = 0.5, -- seconds the zone loop's position takes to follow (no jumps)
}

local function clamp(v, lo, hi)
    if v < lo then return lo end
    if v > hi then return hi end
    return v
end

local function smoothstep(e0, e1, x)
    local t = clamp((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)
end

-- ---- pure helpers (unit tested) ---------------------------------------------------------------

-- Closest point to `p` of a box footprint centred at `c` with world axes `right` and `forward`
-- (unit vectors) and half sizes `hx`, `hz`, on the ground plane (y = p.y). Returns x, z and the
-- distance (0 inside).
function AmbienceZones.closestPoint(p, c, right, forward, hx, hz)
    local dx, dz = p.x - c.x, p.z - c.z
    -- Flatten the axes (a tilted box still has a footprint).
    local rl = math.sqrt(right.x * right.x + right.z * right.z)
    local fl = math.sqrt(forward.x * forward.x + forward.z * forward.z)
    local rx, rz = right.x / math.max(rl, 1e-6), right.z / math.max(rl, 1e-6)
    local fx, fz = forward.x / math.max(fl, 1e-6), forward.z / math.max(fl, 1e-6)
    local lx = clamp(dx * rx + dz * rz, -hx, hx)
    local lz = clamp(dx * fx + dz * fz, -hz, hz)
    local x = c.x + rx * lx + fx * lz
    local z = c.z + rz * lx + fz * lz
    local ex, ez = p.x - x, p.z - z
    return x, z, math.sqrt(ex * ex + ez * ez)
end

-- Volumes and spatial blend at `distance` metres from the nearest zone: open loop volume,
-- zone loop spatial blend.
function AmbienceZones.blend(t, distance)
    -- (A Blend Distance of 0 switches at the zone's edge instead of dividing by zero.)
    local out = smoothstep(0, math.max(t.blendDistance, 0.001), distance)
    local open = t.openVolume * (t.openInZone + (1 - t.openInZone) * out)
    local spatial = smoothstep(0, math.max(t.envelopDistance, 0.001), distance)
    return open, spatial
end

-- ---- the component ----------------------------------------------------------------------------

function AmbienceZones:Start()
    local function source(name)
        local t = self.transform:Find(name)
        if t == nil then return nil end
        return t, t.gameObject:GetComponent("AudioSource")
    end
    self._openT, self._open = source(self.openSource)
    self._zoneT, self._zone = source(self.zoneSource)
    self._zones = {}
    for i = 0, self.transform.childCount - 1 do
        local c = self.transform:GetChild(i)
        local name = c.name
        if string.sub(name, 1, #self.zonePrefix) == self.zonePrefix
            and name ~= self.zoneSource and name ~= self.openSource then
            self._zones[#self._zones + 1] = c
        end
    end
    if self._zone ~= nil then self._zone.volume = self.zoneVolume end
    self._pos = nil
end

function AmbienceZones:Update()
    local cam = Camera.main
    local ear = (cam ~= nil and cam.transform or self.transform).position
    local best, bx, bz = math.huge, ear.x, ear.z
    for _, z in ipairs(self._zones) do
        local s = z.localScale
        local x, zz, d = AmbienceZones.closestPoint(ear, z.position, z.right, z.forward,
            math.abs(s.x) * 0.5, math.abs(s.z) * 0.5)
        if d < best then best, bx, bz = d, x, zz end
    end
    local open, spatial = AmbienceZones.blend(self, best)
    if self._open ~= nil then self._open.volume = open end
    if self._zone ~= nil and best < math.huge then
        local dt = math.max(Time.deltaTime, 0)
        local k = 1 - math.exp(-dt / math.max(self.glideTime, 0.001))
        local p = self._pos
        if p == nil then
            p = { x = bx, z = bz }
        else
            p.x = p.x + (bx - p.x) * k
            p.z = p.z + (bz - p.z) * k
        end
        self._pos = p
        self._zoneT.position = Vector3.new(p.x, ear.y, p.z)
        self._zone.spatialBlend = spatial
    end
end

return AmbienceZones
