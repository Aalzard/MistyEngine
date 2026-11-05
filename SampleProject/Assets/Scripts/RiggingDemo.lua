-- RiggingDemo: moves the IK targets, sways the creature and blends the rig.
-- Space = blend the whole Rig in / out, 1 = arm hint on / off.
local RiggingDemo = {
    speed = 1,       -- target motion speed
    blend = 4,       -- how fast the rig weight eases to its goal
}

function RiggingDemo:Start()
    self._arm = GameObject.Find("Arm Target")
    self._tail = GameObject.Find("Tail Target")
    self._body = GameObject.Find("Creature")
    self._rig = GameObject.Find("Rig"):GetComponent("Rig")
    self._ik = GameObject.Find("Arm IK"):GetComponent("TwoBoneIKConstraint")
    local label = GameObject.Find("State Text")
    self._label = label and label:GetComponent("Text")
    self._goal = 1
end

function RiggingDemo:Update()
    local t = Time.time * self.speed
    -- The arm target circles in front of the shoulder; the circle grows past the arm's reach.
    local r = 0.5 + 0.4 * math.sin(t * 0.45)
    self._arm.transform.position = Vector3.new(
        1.3 + r * math.cos(t), 1.4 + r * math.sin(t * 1.3), -0.7 + 0.3 * math.sin(t * 0.7))
    -- The tail target draws a figure of eight behind the creature.
    self._tail.transform.position = Vector3.new(
        0.8 * math.sin(t * 0.9), 1.45 + 0.35 * math.sin(t * 1.8), 2.0)
    self._body.transform.eulerAngles = Vector3.new(0, 16 * math.sin(t * 0.6), 0)

    if Input.GetKeyDown("space") then self._goal = 1 - self._goal end
    if Input.GetKeyDown("1") then self._ik.hintWeight = 1 - self._ik.hintWeight end
    local w = self._rig.weight
    self._rig.weight = w + (self._goal - w) * math.min(1, Time.deltaTime * self.blend)
    if self._label then
        self._label.text = string.format("Rig weight %.2f    Hint weight %.0f",
            self._rig.weight, self._ik.hintWeight)
    end
end

return RiggingDemo
