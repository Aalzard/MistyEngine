// HLOD: World Partition proxies. The merged objects' colours are in the vertex colours.
// @shader surface
// @property _Smoothness range(0, 1) = 0.08

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    o.albedo = input.vertex_color.rgb;
    o.smoothness = material._Smoothness;
    return o;
}
