// ---- Weather on lit surfaces (render/weather) ----
// `shade` (mesh.wgsl) passes every opaque surface through `weather_surface` after the decals:
// wet surfaces get darker (porous ones most) and glossier, flat open ground collects puddles
// with raindrop ripples, upward faces collect snow, and lightning lights everything open to
// the sky. Glass, water and other transparent surfaces are left alone. Needs weather_common.wgsl.

// Normal xz tilt of raindrop rings on a water film at `xz` (metres) at time `t`, `strength`
// 0..1 (rain intensity: the second layer of rings joins in harder rain).
fn weather_ripples(xz: vec2<f32>, t: f32, strength: f32) -> vec2<f32> {
    var g = vec2<f32>(0.0);
    let layers = select(1, 2, strength > 0.4);
    for (var layer = 0; layer < layers; layer++) {
        let scale = 0.42 + f32(layer) * 0.19;
        let cell = xz / scale + f32(layer) * 17.31;
        let base = floor(cell);
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let c = base + vec2<f32>(f32(dx), f32(dy));
                let h = weather_hash2(c);
                let period = 0.65 + weather_hash2(c + 11.7) * 0.5;
                let tt = t / period + h;
                let n = floor(tt);
                let age = fract(tt) * period;
                // Each drop lands somewhere new in its cell.
                let centre = c + vec2<f32>(weather_hash2(c + n * 1.31), weather_hash2(c + 3.7 + n * 1.71));
                let d = (cell - centre) * scale;
                let dist = length(d);
                let x = dist - age * 0.32;
                let env = exp(-age * 3.2) * (1.0 - smoothstep(0.0, 0.07, abs(x)));
                if (env > 0.0 && dist > 1e-4) {
                    g += d / dist * sin(x * 70.0) * env;
                }
            }
        }
    }
    return g * strength * 0.38;
}

fn weather_surface(world: vec3<f32>, surface: Surface) -> Surface {
    if (!weather_on() || surface.premultiply || surface.alpha < 0.99) {
        return surface;
    }
    let a = weather_surface_at(world);
    if (a.x <= 0.0 && a.z <= 0.0 && weather.flash.w <= 0.0) {
        return surface;
    }
    var s = surface;
    let exposure = weather_exposure(world);
    let n = s.normal;
    let snow = weather_snow_amount(world, n, a.z, exposure);
    // Tops get fully wet, walls less (the water runs off), undersides hardly; nothing under
    // cover.
    let facing = clamp(n.y * 0.5 + 0.5, 0.0, 1.0);
    let wet = a.x * weather.snow_line.w * mix(0.2, 1.0, smoothstep(0.1, 0.8, facing)) * exposure * (1.0 - snow);
    if (wet > 0.0) {
        // Water fills the pores of rough materials (darker); a film makes everything glossy.
        let porous = clamp(1.0 - s.smoothness, 0.0, 1.0) * (1.0 - s.metallic);
        s.albedo *= mix(1.0, 0.42, wet * porous);
        // The film is never even: it stands thick in the hollows of a surface and thin on
        // its high spots, so a wet road shines in patches, not like one mirror.
        let film = weather_fbm(world.xz * 1.9);
        // Soil and turf (very rough surfaces) soak the water up: they darken and stay dull,
        // and hold fewer puddles; sealed surfaces (asphalt, stone, wood) carry the film.
        let sealed = smoothstep(0.04, 0.12, s.smoothness + s.metallic);
        s.smoothness = mix(s.smoothness, max(s.smoothness, mix(0.3, mix(0.58, 0.8, film), sealed)), wet);
        // Puddles on flat open ground.
        let flatness = smoothstep(0.94, 0.99, n.y);
        let fill = a.y * weather.snow_line.z * mix(0.45, 1.0, sealed);
        if (flatness > 0.0 && fill > 0.0) {
            let pattern = weather_fbm(world.xz / max(weather.snow.w, 0.1));
            // Soft edges: a puddle thins out into the wet film round it.
            let puddle = smoothstep(1.0 - fill - 0.06, 1.0 - fill + 0.16, pattern) * flatness * exposure;
            if (puddle > 0.0) {
                s.albedo = mix(s.albedo, s.albedo * 0.3, puddle);
                s.metallic = mix(s.metallic, 0.0, puddle);
                // Not a perfect mirror: rain keeps a puddle's surface stirred, so what it
                // reflects is softened (a sharp mirror also shows every hard edge of the
                // screen-space reflection and the probes).
                s.smoothness = mix(s.smoothness, 0.94, puddle);
                var nn = mix(n, vec3<f32>(0.0, 1.0, 0.0), puddle);
                let rain = weather_precip_at(world).x;
                if (weather.surface.w > 0.5 && rain > 0.0) {
                    let fade = 1.0 - smoothstep(14.0, 32.0, distance(world, view.camera_pos.xyz));
                    if (fade > 0.0) {
                        let strength = clamp(rain / 25.0, 0.15, 1.0);
                        let g = weather_ripples(world.xz, weather.info.y, strength) * fade * puddle;
                        nn += vec3<f32>(g.x, 0.0, g.y);
                    }
                }
                s.normal = normalize(nn);
            }
        }
    }
    if (snow > 0.0) {
        s.albedo = mix(s.albedo, weather.snow.rgb, snow);
        s.metallic = mix(s.metallic, 0.0, snow);
        s.smoothness = mix(s.smoothness, 0.3, snow);
        s.normal = normalize(mix(s.normal, vec3<f32>(0.0, 1.0, 0.0), snow * 0.5));
        s.occlusion = mix(s.occlusion, 1.0, snow * 0.5);
    }
    s.emission += weather_flash_light(world, s.normal, s.albedo, exposure);
    return s;
}
