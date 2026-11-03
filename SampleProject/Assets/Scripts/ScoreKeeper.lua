-- ScoreKeeper: the UIDemo button calls AddPoint (Button ▸ On Click); the score shows in a UI Text.
local ScoreKeeper = {
    scoreText = "Score Text",        -- name of the object with the Text component
    button = "Add Point Button",     -- name of the Button object
    goal = 10,                       -- the button turns off at this score
}

function ScoreKeeper:Start()
    self._score = 0
    self._label = GameObject.Find(self.scoreText):GetComponent("Text")
    local b = GameObject.Find(self.button)
    self._button = b:GetComponent("Button")
    self._image = b:GetComponent("Image")
    self:Show()
end

function ScoreKeeper:AddPoint()
    self._score = self._score + 1
    self:Show()
    if self._score >= self.goal then
        self._button.interactable = false
        self._image.color = Color.new(0.4, 0.8, 0.4)
        self._label.text = "Score: " .. self._score .. "  You win!"
    end
end

function ScoreKeeper:Show()
    self._label.text = "Score: " .. self._score
end

return ScoreKeeper
