-- LocalizationDemo2: English, Türkçe and العربية. Every label is a Localized Text reading
-- Assets/Localization/LocalizationDemo2.strings, so this script only switches the locale
-- (the dropdown) and updates the arguments that change while playing (the star count).
--
-- Things to notice in each language:
--   * Türkçe: "{city:upper}" upper-cases "istanbul" with the Turkish rules: İSTANBUL.
--   * العربية: letters join, lines read right to left, numbers and Latin words inside stay
--     left to right, and the Texts (Mirror for RTL) align to the right.
local LocalizationDemo2 = {}

local LOCALES = { "en", "tr", "ar" }

function LocalizationDemo2:Start()
    self._stars = GameObject.Find("Stars Text"):GetComponent("LocalizedText")
    self._language = GameObject.Find("Language Dropdown"):GetComponent("Dropdown")
    self._count = 3
    for i, code in ipairs(LOCALES) do
        if Localization.locale == code then self._language.value = i - 1 end
    end
    self:ShowStars()
end

function LocalizationDemo2:ShowStars()
    self._stars.args = { self._count }
end

-- The "+1 Star" button: plural forms differ per language (Arabic has six).
function LocalizationDemo2:AddStar()
    self._count = self._count + 1
    self:ShowStars()
end

-- Called by the Language Dropdown's On Value Changed with the option index.
function LocalizationDemo2:LanguageChanged(index)
    local code = LOCALES[index + 1]
    if code then Localization.locale = code end
end

return LocalizationDemo2
