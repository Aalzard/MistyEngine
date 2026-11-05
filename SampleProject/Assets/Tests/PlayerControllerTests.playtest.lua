-- PlayerControllerTests: PlayMode tests of the Platformer2D player
-- (Assets/Scripts/PlayerController2D.lua). Each test gets its own play session.
describe("PlayerController2D", function()
    local player

    beforeEach(function()
        loadScene("Platformer2D")
        player = GameObject.Find("Player")
        assert.notNil(player, "the scene has a Player")
        -- Let it settle on the ground.
        yield(0.5)
    end)

    afterEach(function()
        Input.simulateReset()
    end)

    it("stands on the ground", function()
        assert.near(player.transform.position.y, 0.475, 0.05)
    end)

    it("runs right while D is held", function()
        local start = player.transform.position.x
        Input.simulateKeyDown("d")
        yield(1)
        Input.simulateKeyUp("d")
        expect(player.transform.position.x):toBeGreaterThan(start + 3)
    end)

    it("collects the coins on the way", function()
        Input.simulateKeyDown("d")
        yield(1.2)
        assert.isNil(GameObject.Find("Coin 1"), "Coin 1 collected")
        local label = GameObject.Find("Score Text"):GetComponent("Text")
        assert.equal(label.text, "Coins: 3")
    end)

    it("jumps and lands", function()
        local ground = player.transform.position.y
        Input.simulateKeyPress("space")
        yield(0.3)
        assert.greater(player.transform.position.y, ground + 1.5, "in the air")
        waitUntil(function()
            return math.abs(player.transform.position.y - ground) < 0.05
        end, 5)
    end)
end)
