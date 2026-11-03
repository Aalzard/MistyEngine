-- AudioDemo: switches Audio Mixer snapshots and drives its exposed parameters from the keyboard.
--   1 / 2 / 3   Normal / Muffled (lowpass on Master) / Cave (reverb on SFX) snapshots
--   M           mute or unmute the music (exposed "MusicVol")
--   Up / Down   SFX group volume (exposed "SFXVol")
--   Space       a blip one-shot from the orbiting source
local AudioDemo = {
    fade = 0.75,                     -- snapshot transition time in seconds
}

function AudioDemo:Start()
    self.mixer = AudioMixer.Find("AudioDemo")
    self.hint = GameObject.Find("Hint"):GetComponent("Text")
    self.orbiter = GameObject.Find("Orbiter"):GetComponent("AudioSource")
    self.snapshot = "Normal"
    self.sfx = self.mixer:GetFloat("SFXVol")
    self.musicOff = false
    self:Refresh()
end

function AudioDemo:Go(name)
    self.mixer:FindSnapshot(name):TransitionTo(self.fade)
    self.snapshot = name
end

function AudioDemo:Update()
    if Input.GetKeyDown("1") then self:Go("Normal") end
    if Input.GetKeyDown("2") then self:Go("Muffled") end
    if Input.GetKeyDown("3") then self:Go("Cave") end
    if Input.GetKeyDown("m") then
        self.musicOff = not self.musicOff
        if self.musicOff then
            self.mixer:SetFloat("MusicVol", -80)
        else
            self.mixer:ClearFloat("MusicVol")
        end
    end
    if Input.GetKeyDown("up") then self.sfx = math.min(self.sfx + 3, 20) end
    if Input.GetKeyDown("down") then self.sfx = math.max(self.sfx - 3, -80) end
    self.mixer:SetFloat("SFXVol", self.sfx)
    if Input.GetKeyDown("space") then
        self.orbiter:PlayOneShot("Assets/Audio/Blip.wav", 1)
    end
    self:Refresh()
end

function AudioDemo:Refresh()
    local music = self.musicOff and "off" or "on"
    self.hint.text = "1 Normal   2 Muffled   3 Cave   M music   Up/Down SFX volume   Space blip\n"
        .. string.format("Snapshot: %s    Music: %s    SFX: %+.0f dB", self.snapshot, music, self.sfx)
end

return AudioDemo
