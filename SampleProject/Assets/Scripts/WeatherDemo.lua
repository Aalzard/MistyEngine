-- WeatherDemo: keys switch the weather (4 s blends), L strikes lightning, 0 hands the
-- weather back to the Weather volume's timeline.
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
    Debug.Log("Weather keys: 1 Clear, 2 Drizzle, 3 Rain, 4 Downpour, 5 Thunderstorm, 6 Snow, 7 Blizzard, 8 Sleet, L lightning, 0 timeline")
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
    if Input.GetKeyDown("0") then
        Weather.ClearState(self.blend)
    end
end

function Demo:OnWeatherChanged(state, seconds)
    Debug.Log("Weather: " .. state)
end

return Demo
