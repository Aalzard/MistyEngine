-- LapTimer: times laps of the car round the track's spline. The clock starts when the car
-- first crosses the start line (the spline's first knot); a lap counts once the car has
-- passed every checkpoint (evenly spread along the spline) and crosses the line again.
local LapTimer = {
    car = "Car",
    track = "Track",
    checkpoints = 4,
    lapText = "Lap Text",
    timesText = "Times Text",
}

local function text(name)
    local go = GameObject.Find(name)
    if go ~= nil then
        return go:GetComponent("Text")
    end
    return nil
end

-- 83.456 seconds -> "1:23.456"
local function clock(seconds)
    if seconds == nil then
        return "-:--.---"
    end
    local minutes = math.floor(seconds / 60)
    return string.format("%d:%06.3f", minutes, seconds - minutes * 60)
end

function LapTimer:Start()
    self._car = GameObject.Find(self.car).transform
    self._spline = GameObject.Find(self.track):GetComponent("SplineContainer")
    self._lapLabel = text(self.lapText)
    self._timesLabel = text(self.timesText)
    self.lap = 0
    self.lapsDone = 0
    self.lastLap = nil
    self.bestLap = nil
    self._passed = {}
    local _, t = self._spline:GetNearestPoint(self._car.position)
    self._t = t
    self:Show()
end

function LapTimer:Show()
    if self._lapLabel ~= nil then
        if self.lap == 0 then
            self._lapLabel.text = "Cross the line to start"
        else
            self._lapLabel.text = string.format("LAP %d   %s", self.lap,
                clock(Time.time - self._lapStart))
        end
    end
    if self._timesLabel ~= nil then
        local shown = "LAST " .. clock(self.lastLap) .. "   BEST " .. clock(self.bestLap)
        if shown ~= self._shownTimes then
            self._timesLabel.text = shown
            self._shownTimes = shown
        end
    end
end

function LapTimer:Update()
    local _, t = self._spline:GetNearestPoint(self._car.position)
    local before = self._t
    self._t = t
    -- Checkpoints: the car passed spline position k / (checkpoints + 1) going forwards.
    for k = 1, self.checkpoints do
        local at = k / (self.checkpoints + 1)
        if before < at and t >= at and t - before < 0.2 then
            self._passed[k] = true
        end
    end
    if before > 0.8 and t < 0.2 then
        -- Forwards over the line.
        local all = true
        for k = 1, self.checkpoints do
            if not self._passed[k] then all = false end
        end
        if self.lap > 0 and all then
            local time = Time.time - self._lapStart
            self.lastLap = time
            self.lapsDone = self.lapsDone + 1
            if self.bestLap == nil or time < self.bestLap then
                self.bestLap = time
                local car = GameObject.Find(self.car):GetComponent("RaceCar")
                if car ~= nil then
                    car:Say("Best lap " .. clock(time))
                end
            end
        end
        if self.lap == 0 or all then
            self.lap = self.lap + 1
            self._lapStart = Time.time
            self._passed = {}
        end
    elseif before < 0.2 and t > 0.8 then
        -- Backwards over the line: the checkpoints have to be passed again.
        self._passed = {}
    end
    self:Show()
end

return LapTimer
