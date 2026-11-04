// Wobble: vertices pushed along their normals by moving waves (vertex displacement).
// @shader surface
// @property _ColorA color "Color A" = (1.0, 0.45, 0.2, 1)
// @property _ColorB color "Color B" = (0.95, 0.2, 0.55, 1)
// @property _Amount range(0, 0.3) = 0.08 // How far vertices move
// @property _Frequency range(1, 20) = 7
// @property _Speed range(0, 6) = 2
// @property _Smoothness range(0, 1) = 0.7

fn wave(p: vec3<f32>) -> f32 {
    return sin(p.y * material._Frequency + globals.time.x * material._Speed)
        * cos(p.x * material._Frequency * 0.7 - globals.time.x * material._Speed * 0.6);
}

fn vertex(v: VertexData) -> VertexData {
    var o = v;
    o.position += v.normal * wave(v.position) * material._Amount;
    return o;
}

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let t = 0.5 + 0.5 * sin(input.world_pos.y * 3.0 + input.time);
    o.albedo = mix(material._ColorA.rgb, material._ColorB.rgb, t);
    o.smoothness = material._Smoothness;
    return o;
}
