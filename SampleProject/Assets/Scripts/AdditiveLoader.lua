-- Adds a scene next to this one when it isn't open yet (play the level on its own and the
-- lighting / player scene comes with it).
local AdditiveLoader = { scene = "MultiSceneLighting" }

function AdditiveLoader:Start()
    if SceneManager.GetSceneByName(self.scene) == nil then
        SceneManager.LoadScene(self.scene, LoadSceneMode.Additive)
    end
end

return AdditiveLoader
