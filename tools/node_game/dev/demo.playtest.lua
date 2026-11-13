local L = {}
local function say(...)
    local a = { ... }
    for i = 1, #a do a[i] = tostring(a[i]) end
    L[#L + 1] = table.concat(a, " ")
end
local function pos(name)
    local o = GameObject.Find(name)
    if o == nil then return name .. "=nil" end
    local p = o.transform.position
    return string.format("%s=(%.2f, %.2f)", name, p.x, p.y)
end
describe("Demo", function()
    it("autopilot", function()
        setTimeout(200)
        loadScene("NodeGameStage1")
        for i = 1, 300 do
            yieldFrames(20)
            local h = GameObject.Find("Hearts Text")
            say(i, SceneManager.GetActiveScene().name, pos("Player"), h and h:GetComponent("Text").text, pos("Brute 1"), GameObject.Find("Brute 1") and GameObject.Find("Brute 1"):GetVariable("Health"))
            if SceneManager.GetActiveScene().name == "NodeGameWin" then break end
        end
        assert.fail("\n" .. table.concat(L, "\n"))
    end)
end)
