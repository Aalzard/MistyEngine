-- WeatherDemo: keys switch the weather (4 s blends), L strikes lightning, N turns day into
-- night and back (the sun goes under the horizon: the lamps and the lightning light the
-- rain), 0 hands the weather back to the Weather volume's timeline.
local Demo = {}

local keys = {
    ["1"] = "Clear",
    ["2"] = "Drizzle",
    ["3"] = "Rain",
    ["4"] = "Downpour",
    ["5"] = "Thunderstorm",
    ["6"] = "Snow",
    ["7"] = "Blizzard",
    ["8"] = "Sleet",
}

function Demo:Start()
    self.blend = self.blend or 4
    self.night = false
    local sun = GameObject.Find("Directional Light")
    if sun then
        self.sun = sun.transform
        self.day = self.sun.eulerAngles
    end
    Debug.Log("Weather keys: 1 Clear, 2 Drizzle, 3 Rain, 4 Downpour, 5 Thunderstorm, 6 Snow, 7 Blizzard, 8 Sleet, L lightning, N night / day, 0 timeline")
end

function Demo:Update()
    for key, state in pairs(keys) do
        if Input.GetKeyDown(key) then
            Weather.SetState(state, self.blend)
        end
    end
    if Input.GetKeyDown("l") then
        Weather.Strike()
    end
    if Input.GetKeyDown("n") and self.sun then
        self.night = not self.night
        if self.night then
            -- The sun 28 degrees under the horizon: the moon and the night sky take over.
            self.sun.eulerAngles = Vector3.new(-28, self.day.y, 0)
        else
            self.sun.eulerAngles = self.day
        end
    end
    if Input.GetKeyDown("0") then
        Weather.ClearState(self.blend)
    end
end

function Demo:OnWeatherChanged(state, seconds)
    Debug.Log("Weather: " .. state)
end

return Demo
