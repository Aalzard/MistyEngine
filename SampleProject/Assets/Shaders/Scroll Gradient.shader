// Scroll Gradient: unlit stripes of two colours sliding across the surface.
// @shader unlit
// @property _ColorA color "Color A" = (0.1, 0.9, 0.6, 1)
// @property _ColorB color "Color B" = (0.15, 0.25, 0.95, 1)
// @property _Stripes range(1, 30) = 6
// @property _Speed range(-3, 3) = 0.5 // Stripes per second

fn fragment(input: SurfaceInput) -> vec4<f32> {
    let s = fract((input.uv.x + input.uv.y) * material._Stripes * 0.5 - input.time * material._Speed);
    let t = smoothstep(0.35, 0.5, s) - smoothstep(0.85, 1.0, s);
    let c = mix(material._ColorA.rgb, material._ColorB.rgb, t);
    return vec4<f32>(apply_fog(c, input.world_pos), 1.0);
}
