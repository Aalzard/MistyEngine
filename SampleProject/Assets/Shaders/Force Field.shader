// Force Field: bends the view of what is behind it and glows where it touches the scene.
// Transparent queue, so it can read the opaque scene's colour and depth.
// @shader surface
// @blend premultiply
// @property _Color color "Color" = (0.4, 0.7, 1.0, 1)
// @property _Distortion range(0, 0.1) = 0.03
// @property _EdgeWidth range(0.05, 2) "Contact Width" = 0.4 // Glow band where it meets geometry (m)

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let rim = pow(1.0 - abs(dot(input.world_normal, input.view_dir)), 3.0);
    let wobble = vec2<f32>(sin(input.world_pos.y * 9.0 + input.time * 2.0), cos(input.world_pos.x * 9.0 + input.time * 1.7));
    let behind = scene_color_at(input.screen_uv + wobble * material._Distortion * (0.3 + rim));
    let gap = scene_distance(input.screen_uv) - distance(input.world_pos, view.camera_pos.xyz);
    let contact = 1.0 - clamp(gap / material._EdgeWidth, 0.0, 1.0);
    o.albedo = vec3<f32>(0.0);
    o.smoothness = 0.95;
    o.emission = behind * 0.9 + material._Color.rgb * (rim * 1.5 + contact * 3.0);
    o.alpha = 0.0;
    return o;
}
