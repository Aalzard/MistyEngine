-- RaceSettings: change the game while playing, with the keys listed in the settings panel.
-- C: next car. G: gravity (low, normal, high). 1/2: mass. 3/4: grip. 5/6: top speed.
-- Tab: hide or show the panel. The car's own values live in RaceCar.lua.
local RaceSettings = {
    car = "Car",
    panelText = "Settings Text",
    massStep = 100, -- kg
    gripStep = 0.1,
    speedStep = 10, -- km/h
}

-- Gravity presets (m/s²).
local GRAVITY = {
    { label = "Low", value = 3.7 },
    { label = "Normal", value = 9.81 },
    { label = "High", value = 18 },
}

function RaceSettings:Start()
    self._car = GameObject.Find(self.car):GetComponent("RaceCar")
    local go = GameObject.Find(self.panelText)
    if go ~= nil then
        self._panel = go:GetComponent("Text")
    end
    self.gravity = 2
    self.visible = true
    self._dirty = true
end

function RaceSettings:SetGravity(index)
    self.gravity = ((index - 1) % #GRAVITY) + 1
    Physics.gravity = Vector3.new(0, -GRAVITY[self.gravity].value, 0)
    self._dirty = true
end

function RaceSettings:Show()
    if self._panel == nil then
        return
    end
    if not self.visible then
        self._panel.text = "[Tab] Settings"
        return
    end
    local car = self._car
    local g = GRAVITY[self.gravity]
    self._panel.text = table.concat({
        "SETTINGS                [Tab] hide",
        string.format("[C]    Car         %s", car:ModelName()),
        string.format("[G]    Gravity     %s (%.2f g)", g.label, g.value / 9.81),
        string.format("[1/2]  Mass        %d kg", math.floor(car.mass + 0.5)),
        string.format("[3/4]  Grip        %.1f", car.grip),
        string.format("[5/6]  Top speed   %d km/h", math.floor(car.topSpeed + 0.5)),
        "",
        "[W/S] drive  [A/D] steer  [Shift] boost",
        "[Space] handbrake  [R] reset",
    }, "\n")
end

function RaceSettings:Update()
    local car = self._car
    if car == nil or car.mass == nil then
        return -- the car isn't set up yet
    end
    local tuned = false
    if Input.GetKeyDown("c") then
        car:SetModel(car.model + 1)
        self._dirty = true
    end
    if Input.GetKeyDown("g") then
        self:SetGravity(self.gravity + 1)
    end
    if Input.GetKeyDown("1") then
        car.mass = math.max(300, car.mass - self.massStep)
        tuned = true
    end
    if Input.GetKeyDown("2") then
        car.mass = math.min(4000, car.mass + self.massStep)
        tuned = true
    end
    if Input.GetKeyDown("3") then
        car.grip = math.max(0.3, car.grip - self.gripStep)
        tuned = true
    end
    if Input.GetKeyDown("4") then
        car.grip = math.min(3, car.grip + self.gripStep)
        tuned = true
    end
    if Input.GetKeyDown("5") then
        car.topSpeed = math.max(60, car.topSpeed - self.speedStep)
        tuned = true
    end
    if Input.GetKeyDown("6") then
        car.topSpeed = math.min(400, car.topSpeed + self.speedStep)
        tuned = true
    end
    if Input.GetKeyDown("tab") then
        self.visible = not self.visible
        self._dirty = true
    end
    if tuned then
        car:ApplyTuning()
        self._dirty = true
    end
    if self._dirty then
        self:Show()
        self._dirty = false
    end
end

return RaceSettings
