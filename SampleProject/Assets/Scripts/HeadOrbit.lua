-- HeadOrbit: circles the listener's head. With Spatialize on its Audio Source the sound is
-- rendered binaurally (time and level differences, head shadow): on headphones it goes round
-- your head, and behind you sounds duller than in front.
local HeadOrbit = {
    radius = 2.5,
    speed = 1.2,                     -- radians per second
}

function HeadOrbit:Start()
    self.ear = GameObject.Find("Main Camera").transform
end

function HeadOrbit:Update()
    local a = Time.time * self.speed
    local c = self.ear.position
    self.transform.position = Vector3.new(c.x + Mathf.Cos(a) * self.radius, c.y,
        c.z + Mathf.Sin(a) * self.radius)
end

return HeadOrbit
