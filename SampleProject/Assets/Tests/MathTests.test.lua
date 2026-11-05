-- MathTests: EditMode tests of plain math (they need no scene). Run them from
-- Window ▸ General ▸ Test Runner, or `misty --run-tests SampleProject`.
describe("Mathf", function()
    it("clamps", function()
        assert.equal(Mathf.Clamp(5, 0, 3), 3)
        assert.equal(Mathf.Clamp(-1, 0, 3), 0)
        assert.equal(Mathf.Clamp01(0.25), 0.25)
    end)

    it("lerps with t clamped to 0..1", function()
        assert.near(Mathf.Lerp(0, 10, 0.5), 5)
        assert.equal(Mathf.Lerp(0, 10, 2), 10)
    end)

    it("converts degrees to radians", function()
        assert.near(90 * Mathf.Deg2Rad, Mathf.PI / 2, 1e-9)
        assert.near(Mathf.Sin(30 * Mathf.Deg2Rad), 0.5, 1e-6)
    end)
end)

describe("Vector3", function()
    it("adds and scales", function()
        local v = Vector3.new(1, 2, 3) + Vector3.up * 2
        assert.near(v, Vector3.new(1, 4, 3), 1e-6)
    end)

    it("measures lengths and angles", function()
        assert.near(Vector3.new(3, 4, 0).magnitude, 5, 1e-6)
        assert.near(Vector3.Distance(Vector3.zero, Vector3.new(0, 0, 2)), 2, 1e-6)
        assert.equal(Vector3.Dot(Vector3.right, Vector3.up), 0)
    end)

    it("crosses right and up into forward", function()
        expect(Vector3.Cross(Vector3.right, Vector3.up)):toBeNear(Vector3.forward, 1e-6)
    end)

    it("normalizes", function()
        expect(Vector3.new(0, 0, 9).normalized):toBeNear(Vector3.forward, 1e-6)
    end)
end)
