-- VehicleTests: PlayMode tests of the VehicleDemo car (Assets/Scripts/CarController.lua).
describe("CarController", function()
    local car, body

    beforeEach(function()
        loadScene("VehicleDemo")
        car = GameObject.Find("Car")
        assert.notNil(car, "the scene has a Car")
        body = car:GetComponent("Rigidbody")
        -- Let it settle on its wheels.
        yield(1)
    end)

    afterEach(function()
        Input.simulateReset()
    end)

    it("rests on its wheels", function()
        assert.less(body.velocity.magnitude, 0.2, "at rest")
        local wheel = GameObject.Find("Wheel FL"):GetComponent("WheelCollider")
        assert.isTrue(wheel.isGrounded)
    end)

    it("drives forward on the throttle", function()
        local start = car.transform.position
        local forward = car.transform.forward
        Input.simulateKeyDown("w")
        yield(2)
        local moved = car.transform.position - start
        assert.greater(Vector3.Dot(moved, forward), 5, "moved forward")
        assert.greater(body.velocity.magnitude, 3, "still rolling")
    end)

    it("brakes to a stop with the handbrake", function()
        Input.simulateKeyDown("w")
        yield(1.5)
        Input.simulateKeyUp("w")
        Input.simulateKeyDown("space")
        waitUntil(function()
            return body.velocity.magnitude < 0.3
        end, 8)
    end)
end)
