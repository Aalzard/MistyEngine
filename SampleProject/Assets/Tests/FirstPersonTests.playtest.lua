-- FirstPersonTests: PlayMode tests of the First Person Player (Assets/Prefabs/First Person
-- Player.prefab with FirstPersonController.lua and the head bob scripts) on the FirstPersonDemo
-- course. Each test gets its own play session at a fixed 60 Hz.
describe("First Person Player", function()
    local player, head, camera

    -- Put the player's feet at a point of the course, facing +z.
    local function teleport(x, y, z)
        player.transform.position = Vector3.new(x, y, z)
        player.transform.eulerAngles = Vector3.new(0, 0, 0)
        yield(0.3)
    end

    beforeEach(function()
        loadScene("FirstPersonDemo")
        player = GameObject.Find("Player")
        head = GameObject.Find("HeadBobVertical")
        camera = GameObject.Find("Camera")
        assert.notNil(player, "the scene has the First Person Player")
        yield(0.5)
    end)

    afterEach(function()
        Input.simulateReset()
    end)

    it("stands on the floor and walks forward", function()
        assert.near(player.transform.position.y, 0, 0.1)
        local z = player.transform.position.z
        Input.simulateKeyDown("w")
        yield(1.5)
        Input.simulateKeyUp("w")
        expect(player.transform.position.z - z):toBeGreaterThan(4)
        expect(player.transform.position.z - z):toBeLessThan(6.5)
    end)

    it("runs faster with Shift and widens the field of view", function()
        local cam = camera:GetComponent("Camera")
        local fov = cam.fieldOfView
        local z = player.transform.position.z
        Input.simulateKeyDown("w")
        Input.simulateKeyDown("left shift")
        yield(1.5)
        expect(player.transform.position.z - z):toBeGreaterThan(7)
        expect(cam.fieldOfView):toBeGreaterThan(fov + 4)
        Input.simulateReset()
        yield(1)
        assert.near(cam.fieldOfView, fov, 0.5)
    end)

    it("bobs the head while walking and settles when stopped", function()
        local lo, hi = 10, -10
        Input.simulateKeyDown("w")
        for _ = 1, 60 do
            yieldFrames(1)
            local y = head.transform.localPosition.y
            lo, hi = math.min(lo, y), math.max(hi, y)
        end
        -- (The tuned walk amplitude is 1.5 cm: about 3 cm from crest to trough.)
        assert.greater(hi - lo, 0.02, "the head moves up and down")
        Input.simulateKeyUp("w")
        yield(1)
        assert.near(head.transform.localPosition.y, 1.7, 0.01)
        assert.near(GameObject.Find("HeadBobHorizontal").transform.localPosition.x, 0, 0.005)
    end)

    it("looks around with the mouse", function()
        Input.simulateMouseDelta(100, 0)
        yieldFrames(2)
        assert.near(player.transform.eulerAngles.y, 20, 1)
        Input.simulateMouseDelta(0, 50)
        yieldFrames(2)
        -- Looking up is a negative pitch (stored as 350 in 0..360 Euler angles).
        local pitch = camera.transform.localEulerAngles.x
        if pitch > 180 then pitch = pitch - 360 end
        assert.near(pitch, -10, 1)
    end)

    it("jumps and lands", function()
        local ground = player.transform.position.y
        Input.simulateKeyPress("space")
        local apex = ground
        for _ = 1, 40 do
            yieldFrames(1)
            apex = math.max(apex, player.transform.position.y)
        end
        assert.greater(apex, ground + 0.9, "jumped about 1.2 m")
        waitUntil(function() return math.abs(player.transform.position.y - ground) < 0.05 end, 3)
    end)

    it("climbs the 15 degree ramp and the stairs", function()
        teleport(-8, 0.1, 0)
        Input.simulateKeyDown("w")
        yield(4)
        Input.simulateKeyUp("w")
        expect(player.transform.position.y):toBeGreaterThan(1.8)

        teleport(8, 0.1, 1)
        Input.simulateKeyDown("w")
        yield(3)
        Input.simulateKeyUp("w")
        expect(player.transform.position.y):toBeGreaterThan(1.5)
    end)

    it("climbs the 30 degree ramp", function()
        teleport(-13, 0.1, 0)
        Input.simulateKeyDown("w")
        yield(4)
        Input.simulateKeyUp("w")
        expect(player.transform.position.y):toBeGreaterThan(2.5)
    end)

    it("jumps from the first platform to the second", function()
        teleport(0, 0.8, 11.8)
        assert.near(player.transform.position.y, 0.75, 0.1)
        Input.simulateKeyDown("w")
        -- Jump at the platform's edge (z = 13); a walking jump clears about 2.8 m.
        waitUntil(function() return player.transform.position.z > 12.75 end, 3)
        Input.simulateKeyPress("space")
        yield(0.75)
        Input.simulateReset()
        yield(0.4)
        local p = player.transform.position
        assert.greater(p.z, 14.6, "crossed the 1.8 m gap")
        assert.less(p.z, 16.9, "stayed on the second platform")
        assert.near(p.y, 0.75, 0.1)
    end)

    it("needs to crouch to get through the low tunnel", function()
        teleport(-8, 0.1, 11)
        Input.simulateKeyDown("w")
        yield(2.5)
        Input.simulateKeyUp("w")
        expect(player.transform.position.z):toBeLessThan(14.5)

        teleport(-8, 0.1, 11)
        Input.simulateKeyDown("left ctrl")
        yield(0.5)
        local cc = player:GetComponent("CharacterController")
        assert.near(cc.height, 1.2, 0.05)
        assert.less(head.transform.localPosition.y, 1.2)
        Input.simulateKeyDown("w")
        yield(5)
        Input.simulateReset()
        expect(player.transform.position.z):toBeGreaterThan(18.5)
        yield(0.6)
        assert.near(cc.height, 1.8, 0.05, "stands up again outside")
    end)

    it("locks the cursor and releases it with Escape", function()
        assert.equal(Cursor.lockState, CursorLockMode.Locked)
        assert.isFalse(Cursor.visible)
        Input.simulateKeyPress("escape")
        yieldFrames(2)
        assert.equal(Cursor.lockState, CursorLockMode.None)
        assert.isTrue(Cursor.visible)
        Input.simulateMouseDown(0)
        yieldFrames(2)
        Input.simulateMouseUp(0)
        assert.equal(Cursor.lockState, CursorLockMode.Locked)
    end)
end)
