-- PlayerController2D: A/D (or the arrow keys) run, Space jumps while standing on something.
-- Ground check: three short rays down from the feet against the Default layer.
-- Coins are triggers: touching one destroys it and updates the "Score Text" UI Text.
local PlayerController2D = {
    speed = 6,
    jumpSpeed = 14,
    groundDistance = 0.1,
}

function PlayerController2D:Start()
    self.rb = self.gameObject:GetComponent("Rigidbody2D")
    self.sprite = self.gameObject:GetComponent("SpriteRenderer")
    self.ground = LayerMask.GetMask("Default")
    self.coins = 0
    self.jump = false
    local label = GameObject.Find("Score Text")
    if label ~= nil then
        self.label = label:GetComponent("Text")
    end
end

function PlayerController2D:IsGrounded()
    local p = self.transform.position
    for _, dx in ipairs({ -0.3, 0, 0.3 }) do
        local feet = Vector2.new(p.x + dx, p.y - 0.45)
        if Physics2D.Raycast(feet, Vector2.down, self.groundDistance, self.ground) ~= nil then
            return true
        end
    end
    return false
end

function PlayerController2D:Update()
    if Input.GetKeyDown("space") then
        self.jump = true
    end
end

function PlayerController2D:FixedUpdate()
    local h = Input.GetAxis("Horizontal")
    local v = self.rb.velocity
    local vy = v.y
    if self.jump and self:IsGrounded() then
        vy = self.jumpSpeed
    end
    self.jump = false
    self.rb.velocity = Vector2.new(h * self.speed, vy)
    if h ~= 0 then
        self.sprite.flipX = h < 0
    end
end

function PlayerController2D:OnTriggerEnter2D(other)
    if other.name:sub(1, 4) == "Coin" then
        Destroy(other.gameObject)
        self.coins = self.coins + 1
        if self.label ~= nil then
            self.label.text = "Coins: " .. self.coins
        end
    end
end

return PlayerController2D
