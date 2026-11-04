// Hologram: see-through, glowing, with scan lines rolling upwards and a bright rim.
// @shader surface
// @blend alpha
// @cull off
// @property _Color color "Color" = (0.25, 0.85, 1.0, 1.0)
// @property _RimPower range(0.5, 8) "Rim Sharpness" = 2.5 // Higher = thinner bright edge
// @property _LineDensity range(1, 80) "Lines per Metre" = 18
// @property _ScrollSpeed range(0, 3) "Scroll Speed" = 0.4 // Metres per second
// @property _Glow range(0, 10) = 2.5 // Emission strength (above 1 blooms)
// @property _Flicker range(0, 1) = 0.15

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let rim = pow(1.0 - abs(dot(input.world_normal, input.view_dir)), material._RimPower);
    let y = (input.world_pos.y - input.time * material._ScrollSpeed) * material._LineDensity;
    let band = smoothstep(0.55, 1.0, 0.5 + 0.5 * sin(y * 6.2831));
    let flicker = 1.0 - material._Flicker * (0.5 + 0.5 * sin(input.time * 37.0) * sin(input.time * 11.0));
    let c = material._Color.rgb;
    o.albedo = c * 0.05;
    o.smoothness = 0.9;
    o.emission = c * (rim * 1.5 + band * 0.5 + 0.1) * material._Glow * flicker;
    o.alpha = clamp((rim + band * 0.4 + 0.08) * material._Color.a, 0.0, 1.0);
    return o;
}
