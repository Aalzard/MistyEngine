-- PingPong: slides the object back and forth along an axis, pausing at each end.
local PingPong = {
    axis = Vector3.new(0, 0, 1),
    distance = 12,
    speed = 2,
    pause = 1,
}

function PingPong:Start()
    self.origin = self.transform.position
    self.t = 0
    self.dir = 1
    self.wait = 0
end

function PingPong:Update()
    if self.wait > 0 then
        self.wait = self.wait - Time.deltaTime
        return
    end
    self.t = self.t + self.dir * self.speed * Time.deltaTime / self.distance
    if self.t >= 1 then
        self.t = 1
        self.dir = -1
        self.wait = self.pause
    elseif self.t <= 0 then
        self.t = 0
        self.dir = 1
        self.wait = self.pause
    end
    self.transform.position = self.origin + self.axis * (self.t * self.distance)
end

return PingPong
