-- CoinCounter: coins collected with the "+1 Coin" button, shown by a Localized Text whose
-- string has plural forms ("1 coin", "2 coins", "2 pièces"…). The count is kept in save
-- games through OnSave / OnLoad.
local CoinCounter = {
    text = "Coins Text",   -- object with the Localized Text showing the count
    coins = 0,
}

function CoinCounter:Start()
    self._text = GameObject.Find(self.text):GetComponent("LocalizedText")
    self:Show()
end

function CoinCounter:AddCoin()
    self.coins = self.coins + 1
    self:Show()
end

function CoinCounter:Show()
    self._text.args = { self.coins }
end

function CoinCounter:OnSave(data)
    data.coins = self.coins
end

-- Runs after Awake and before Start, so Start shows the loaded count.
function CoinCounter:OnLoad(data)
    self.coins = data.coins or 0
end

return CoinCounter
