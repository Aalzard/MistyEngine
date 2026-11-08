-- AudioDemo2: blends the Outdoor and Indoor mixer snapshots by how far into the cave the
-- listener is (TransitionToSnapshots with weights), and drives the demo from the keyboard.
--   D       radio dialogue on / off (it ducks the music through a send)
--   Space   a clap from the orbiting buzzer (its SFX send feeds the reverb bus)
--   M       music on / off (exposed "MusicVol")
--   W / S   walk in / out by hand (otherwise the camera walks on its own)
local AudioDemo2 = {
    inner = 3.5,                     -- fully Indoor inside this distance from the cave centre
    outer = 8.0,                     -- fully Outdoor beyond this one
    cave = Vector3.new(0, 1, 8),
}

function AudioDemo2:Start()
    self.mixer = AudioMixer.Find("AudioDemo2")
    self.outdoor = self.mixer:FindSnapshot("Outdoor")
    self.indoor = self.mixer:FindSnapshot("Indoor")
    self.ear = GameObject.Find("Main Camera").transform
    self.radio = GameObject.Find("Radio"):GetComponent("AudioSource")
    self.buzzer = GameObject.Find("Buzzer"):GetComponent("AudioSource")
    self.hint = GameObject.Find("Hint"):GetComponent("Text")
    self.weight = -1
    self.talking = true
    self.musicOff = false
end

function AudioDemo2:Update()
    local d = Vector3.Distance(self.ear.position, self.cave)
    local w = Mathf.Clamp01((self.outer - d) / (self.outer - self.inner))
    if math.abs(w - self.weight) > 0.005 then
        self.mixer:TransitionToSnapshots({ self.outdoor, self.indoor }, { 1 - w, w }, 0.1)
        self.weight = w
    end
    if Input.GetKeyDown("d") then
        self.talking = not self.talking
        if self.talking then self.radio:Play() else self.radio:Stop() end
    end
    if Input.GetKeyDown("space") then
        self.buzzer:PlayOneShot("Assets/Audio/Clap.wav", 1)
    end
    if Input.GetKeyDown("m") then
        self.musicOff = not self.musicOff
        if self.musicOff then self.mixer:SetFloat("MusicVol", -80) else self.mixer:ClearFloat("MusicVol") end
    end
    self.hint.text = "D dialogue   Space clap   M music   W/S walk\n"
        .. string.format("Outdoor %.2f   Indoor %.2f   Dialogue: %s   Music: %s",
            1 - w, w, self.talking and "on" or "off", self.musicOff and "off" or "on")
end

return AudioDemo2
