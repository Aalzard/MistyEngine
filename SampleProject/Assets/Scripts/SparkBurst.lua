-- Press Space to throw extra sparks; the count shows how the API works.
local SparkBurst = { amount = 40 }

function SparkBurst:Start()
    self.sparks = self.gameObject:GetComponent("ParticleSystem")
end

function SparkBurst:Update()
    if Input.GetKeyDown("space") then
        self.sparks:Emit(self.amount)
        Debug.Log("Sparks alive:", self.sparks.particleCount)
    end
end

return SparkBurst
