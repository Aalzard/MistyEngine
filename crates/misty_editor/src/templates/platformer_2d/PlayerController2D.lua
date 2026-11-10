-- PlayerController2D: run with A/D or the arrow keys, jump with Space (or W / Up).
-- Hold the jump key for a full jump, tap it for a small hop. A jump pressed just before
-- landing, or just after running off a ledge, still counts.
-- Ground check: three short rays down from the feet against the Default layer.
-- Coins are triggers: touching one destroys it and updates the "Score Text" UI Text.
local PlayerController2D = {
    speed = 6,
    jumpSpeed = 14,
    groundDistance = 0.1,
    -- Seconds a jump press is remembered before landing.
    jumpBuffer = 0.12,
    -- Seconds after leaving the ground that a jump still works.
    coyoteTime = 0.1,
    -- Share of the upward speed kept when the jump key is let go early.
    jumpCut = 0.5,
}

local JUMP_KEYS = { "space", "w", "up" }

local function jumpHeld()
    for _, key in ipairs(JUMP_KEYS) do
        if Input.GetKey(key) then
            return true
        end
    end
    return false
end

function PlayerController2D:Start()
    self.rb = self.gameObject:GetComponent("Rigidbody2D")
    self.sprite = self.gameObject:GetComponent("SpriteRenderer")
    self.ground = LayerMask.GetMask("Default")
    self.coins = 0
    self.jumpPressedAt = -1
    self.groundedAt = -1
    self.rising = false
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
    for _, key in ipairs(JUMP_KEYS) do
        if Input.GetKeyDown(key) then
            self.jumpPressedAt = Time.time
        end
    end
end

function PlayerController2D:FixedUpdate()
    local now = Time.time
    if self:IsGrounded() then
        self.groundedAt = now
    end
    local h = Input.GetAxis("Horizontal")
    local vy = self.rb.velocity.y
    local buffered = self.jumpPressedAt >= 0 and now - self.jumpPressedAt <= self.jumpBuffer
    local onGround = self.groundedAt >= 0 and now - self.groundedAt <= self.coyoteTime
    if buffered and onGround then
        vy = self.jumpSpeed
        self.jumpPressedAt = -1
        self.groundedAt = -1
        self.rising = true
    elseif self.rising and vy > 0 and not jumpHeld() then
        -- Let go early: a lower jump.
        vy = vy * self.jumpCut
        self.rising = false
    end
    if vy <= 0 then
        self.rising = false
    end
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
