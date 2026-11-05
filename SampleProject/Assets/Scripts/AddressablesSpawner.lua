-- AddressablesSpawner: the AddressablesDemo buttons load prefabs from the Asset Groups while
-- playing. "Spawn by Address" instantiates one address with Assets.InstantiateAsync; "Spawn by
-- Label" loads every asset with a label (Assets.LoadAsync awaited in a coroutine) and
-- instantiates them; "Release All" releases every handle (InstantiateAsync objects are
-- destroyed, bundles nothing needs unload). The stats line shows the loader's counters.
local AddressablesSpawner = {
    address = "Creatures/Orb",
    label = "props",
    statsText = "Stats Text",
}

function AddressablesSpawner:Start()
    self._handles = {}
    self._spawned = {}
    self._count = 0
    self._label = GameObject.Find(self.statsText):GetComponent("Text")
    -- One of each right away, so the demo shows something before any click.
    self:SpawnByAddress()
    self:SpawnByLabel()
end

-- Rows of six in front of the camera.
function AddressablesSpawner:Place(go)
    local i = self._count
    self._count = self._count + 1
    go.transform.position = Vector3.new((i % 6) * 1.6 - 4, 0.6, math.floor(i / 6) * 1.6 - 1)
end

function AddressablesSpawner:SpawnByAddress()
    local h = Assets.InstantiateAsync(self.address)
    table.insert(self._handles, h)
    h:OnCompleted(function(h)
        if h.status == "Succeeded" then
            self:Place(h.result)
        else
            Debug.LogWarning(h.error)
        end
    end)
end

function AddressablesSpawner:SpawnByLabel()
    Assets.Run(function()
        local h = Assets.LoadAsync(self.label):Await()
        table.insert(self._handles, h)
        if h.status ~= "Succeeded" then
            Debug.LogWarning(h.error)
            return
        end
        for _, path in ipairs(h.results) do
            local go = Instantiate(path)
            table.insert(self._spawned, go)
            self:Place(go)
        end
    end)
end

function AddressablesSpawner:ReleaseAll()
    for _, go in ipairs(self._spawned) do
        Destroy(go)
    end
    for _, h in ipairs(self._handles) do
        Assets.Release(h)
    end
    self._spawned, self._handles, self._count = {}, {}, 0
end

function AddressablesSpawner:Update()
    local s = Assets.GetStats()
    self._label.text = string.format(
        "Loaded: %d   Released: %d   Live handles: %d   Bundles: %d",
        s.loaded, s.released, s.handles, s.bundles)
end

return AddressablesSpawner
