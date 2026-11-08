//! A tiny binary FBX 7.4 writer, used to make the sample FBX assets and the import tests'
//! files in code (no downloaded or third-party FBX files).
//!
//! Only what those files need: node records with scalar, string, raw and array properties,
//! plus helpers for the common object kinds (models, geometry, materials, skins, blend shapes
//! and animation curves). Arrays are stored uncompressed.

/// FBX time units per second.
pub const KTIME: i64 = 46_186_158_000;

/// A skin cluster: bone model, vertex indices, weights, bone world matrix at bind, mesh world
/// matrix at bind.
pub type Cluster = (i64, Vec<i32>, Vec<f64>, glam::DMat4, glam::DMat4);

/// A blend shape: name, vertex indices, position offsets.
pub type Shape<'a> = (&'a str, Vec<i32>, Vec<[f64; 3]>);

/// One property of a node record.
#[derive(Clone, Debug)]
pub enum Prop {
    I32(i32),
    I64(i64),
    F64(f64),
    Str(String),
    Raw(Vec<u8>),
    F32s(Vec<f32>),
    F64s(Vec<f64>),
    I32s(Vec<i32>),
    I64s(Vec<i64>),
}

impl From<&str> for Prop {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}
impl From<String> for Prop {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}
impl From<i32> for Prop {
    fn from(v: i32) -> Self {
        Self::I32(v)
    }
}
impl From<i64> for Prop {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}
impl From<f64> for Prop {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

/// A node record: name, properties, children.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(name: &str, props: Vec<Prop>) -> Self {
        Self {
            name: name.to_owned(),
            props,
            children: Vec::new(),
        }
    }

    pub fn child(mut self, c: Node) -> Self {
        self.children.push(c);
        self
    }

    pub fn push(&mut self, c: Node) {
        self.children.push(c);
    }

    fn write(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0u8; 12]); // end offset, property count, property bytes
        out.push(self.name.len() as u8);
        out.extend_from_slice(self.name.as_bytes());
        let props_start = out.len();
        for p in &self.props {
            write_prop(p, out);
        }
        let props_len = (out.len() - props_start) as u32;
        for c in &self.children {
            c.write(out);
        }
        if !self.children.is_empty() || self.props.is_empty() {
            out.extend_from_slice(&[0u8; 13]);
        }
        let end = out.len() as u32;
        out[start..start + 4].copy_from_slice(&end.to_le_bytes());
        out[start + 4..start + 8].copy_from_slice(&(self.props.len() as u32).to_le_bytes());
        out[start + 8..start + 12].copy_from_slice(&props_len.to_le_bytes());
    }
}

fn array<T: Copy>(code: u8, v: &[T], bytes: impl Fn(T) -> Vec<u8>, out: &mut Vec<u8>) {
    let data: Vec<u8> = v.iter().flat_map(|x| bytes(*x)).collect();
    out.push(code);
    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // not compressed
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
}

fn write_prop(p: &Prop, out: &mut Vec<u8>) {
    match p {
        Prop::I32(v) => {
            out.push(b'I');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::I64(v) => {
            out.push(b'L');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::F64(v) => {
            out.push(b'D');
            out.extend_from_slice(&v.to_le_bytes());
        }
        Prop::Str(s) => {
            out.push(b'S');
            out.extend_from_slice(&(s.len() as u32).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        Prop::Raw(r) => {
            out.push(b'R');
            out.extend_from_slice(&(r.len() as u32).to_le_bytes());
            out.extend_from_slice(r);
        }
        Prop::F32s(v) => array(b'f', v, |x| x.to_le_bytes().to_vec(), out),
        Prop::F64s(v) => array(b'd', v, |x| x.to_le_bytes().to_vec(), out),
        Prop::I32s(v) => array(b'i', v, |x| x.to_le_bytes().to_vec(), out),
        Prop::I64s(v) => array(b'l', v, |x| x.to_le_bytes().to_vec(), out),
    }
}

/// `"Name\0\x01Class"`, the binary FBX object name.
pub fn name(n: &str, class: &str) -> String {
    format!("{n}\0\x01{class}")
}

/// A `P` record of a `Properties70` block.
pub fn p(name: &str, ty: &str, label: &str, values: Vec<Prop>) -> Node {
    let mut props: Vec<Prop> = vec![name.into(), ty.into(), label.into(), "A".into()];
    props.extend(values);
    Node::new("P", props)
}

pub fn p_vec(name: &str, ty: &str, v: [f64; 3]) -> Node {
    p(name, ty, "", v.iter().map(|x| Prop::F64(*x)).collect())
}

pub fn props70(ps: Vec<Node>) -> Node {
    Node {
        name: "Properties70".into(),
        props: Vec::new(),
        children: ps,
    }
}

/// Scene axes and units of a file.
#[derive(Clone, Copy, Debug)]
pub struct Axes {
    /// 1 = Y up, 2 = Z up.
    pub up: i32,
    /// Centimetres per file unit (1 = centimetres, 100 = metres).
    pub unit_scale: f64,
}

/// A whole file being built: objects and connections.
#[derive(Default)]
pub struct Document {
    pub objects: Vec<Node>,
    pub connections: Vec<Node>,
    next_id: i64,
    pub stop_seconds: f64,
}

impl Document {
    pub fn new() -> Self {
        Self {
            next_id: 1_000_000,
            ..Default::default()
        }
    }

    pub fn id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }

    /// Child `src` connected to parent `dst`.
    pub fn connect(&mut self, src: i64, dst: i64) {
        self.connections
            .push(Node::new("C", vec!["OO".into(), src.into(), dst.into()]));
    }

    /// Child `src` connected to property `prop` of `dst`.
    pub fn connect_prop(&mut self, src: i64, dst: i64, prop: &str) {
        self.connections.push(Node::new(
            "C",
            vec!["OP".into(), src.into(), dst.into(), prop.into()],
        ));
    }

    /// A Model (`kind` = "Mesh", "LimbNode" or "Null") under `parent` (0 = scene root).
    pub fn model(
        &mut self,
        n: &str,
        kind: &str,
        parent: i64,
        t: [f64; 3],
        r: [f64; 3],
        s: [f64; 3],
    ) -> i64 {
        let id = self.id();
        let node = Node::new(
            "Model",
            vec![id.into(), name(n, "Model").into(), kind.into()],
        )
        .child(Node::new("Version", vec![232.into()]))
        .child(props70(vec![
            p_vec("Lcl Translation", "Lcl Translation", t),
            p_vec("Lcl Rotation", "Lcl Rotation", r),
            p_vec("Lcl Scaling", "Lcl Scaling", s),
        ]));
        self.objects.push(node);
        self.connect(id, parent);
        if kind == "LimbNode" {
            let attr = self.id();
            self.objects.push(
                Node::new(
                    "NodeAttribute",
                    vec![
                        attr.into(),
                        name(n, "NodeAttribute").into(),
                        "LimbNode".into(),
                    ],
                )
                .child(Node::new("TypeFlags", vec!["Skeleton".into()])),
            );
            self.connect(attr, id);
        }
        id
    }

    /// Mesh geometry: polygons (vertex index lists), per-polygon-vertex normals and UVs, and a
    /// material index per polygon. Returns the geometry id.
    pub fn geometry(
        &mut self,
        n: &str,
        positions: &[[f64; 3]],
        polygons: &[Vec<i32>],
        normals: &[[f64; 3]],
        uvs: &[[f64; 2]],
        materials: &[i32],
    ) -> i64 {
        let id = self.id();
        let mut index = Vec::new();
        for poly in polygons {
            for (k, v) in poly.iter().enumerate() {
                index.push(if k + 1 == poly.len() { -v - 1 } else { *v });
            }
        }
        let mut g = Node::new(
            "Geometry",
            vec![id.into(), name(n, "Geometry").into(), "Mesh".into()],
        )
        .child(Node::new(
            "Vertices",
            vec![Prop::F64s(positions.iter().flatten().copied().collect())],
        ))
        .child(Node::new("PolygonVertexIndex", vec![Prop::I32s(index)]))
        .child(Node::new("GeometryVersion", vec![124.into()]));
        let mut layer =
            Node::new("Layer", vec![0.into()]).child(Node::new("Version", vec![100.into()]));
        let element = |ty: &str| {
            Node::new("LayerElement", Vec::new())
                .child(Node::new("Type", vec![ty.into()]))
                .child(Node::new("TypedIndex", vec![0.into()]))
        };
        if !normals.is_empty() {
            g.push(
                Node::new("LayerElementNormal", vec![0.into()])
                    .child(Node::new("Version", vec![101.into()]))
                    .child(Node::new("Name", vec!["".into()]))
                    .child(Node::new(
                        "MappingInformationType",
                        vec!["ByPolygonVertex".into()],
                    ))
                    .child(Node::new("ReferenceInformationType", vec!["Direct".into()]))
                    .child(Node::new(
                        "Normals",
                        vec![Prop::F64s(normals.iter().flatten().copied().collect())],
                    )),
            );
            layer.push(element("LayerElementNormal"));
        }
        if !uvs.is_empty() {
            g.push(
                Node::new("LayerElementUV", vec![0.into()])
                    .child(Node::new("Version", vec![101.into()]))
                    .child(Node::new("Name", vec!["UVMap".into()]))
                    .child(Node::new(
                        "MappingInformationType",
                        vec!["ByPolygonVertex".into()],
                    ))
                    .child(Node::new(
                        "ReferenceInformationType",
                        vec!["IndexToDirect".into()],
                    ))
                    .child(Node::new(
                        "UV",
                        vec![Prop::F64s(uvs.iter().flatten().copied().collect())],
                    ))
                    .child(Node::new(
                        "UVIndex",
                        vec![Prop::I32s((0..uvs.len() as i32).collect())],
                    )),
            );
            layer.push(element("LayerElementUV"));
        }
        if !materials.is_empty() {
            g.push(
                Node::new("LayerElementMaterial", vec![0.into()])
                    .child(Node::new("Version", vec![101.into()]))
                    .child(Node::new("Name", vec!["".into()]))
                    .child(Node::new(
                        "MappingInformationType",
                        vec!["ByPolygon".into()],
                    ))
                    .child(Node::new(
                        "ReferenceInformationType",
                        vec!["IndexToDirect".into()],
                    ))
                    .child(Node::new("Materials", vec![Prop::I32s(materials.to_vec())])),
            );
            layer.push(element("LayerElementMaterial"));
        }
        g.push(layer);
        self.objects.push(g);
        id
    }

    /// A Phong material with a diffuse colour, shininess and emission.
    pub fn material(
        &mut self,
        n: &str,
        diffuse: [f64; 3],
        shininess: f64,
        emissive: [f64; 3],
    ) -> i64 {
        let id = self.id();
        self.objects.push(
            Node::new(
                "Material",
                vec![id.into(), name(n, "Material").into(), "".into()],
            )
            .child(Node::new("Version", vec![102.into()]))
            .child(Node::new("ShadingModel", vec!["phong".into()]))
            .child(props70(vec![
                p_vec("DiffuseColor", "Color", diffuse),
                p("DiffuseFactor", "Number", "", vec![1.0.into()]),
                p("Shininess", "Number", "", vec![shininess.into()]),
                p("ShininessExponent", "Number", "", vec![shininess.into()]),
                p_vec("EmissiveColor", "Color", emissive),
                p("EmissiveFactor", "Number", "", vec![1.0.into()]),
            ])),
        );
        id
    }

    /// A file texture on property `slot` ("DiffuseColor", "NormalMap") of `material`;
    /// `content` embeds the image bytes (a Video with Content).
    pub fn texture(
        &mut self,
        material: i64,
        slot: &str,
        file: &str,
        content: Option<Vec<u8>>,
    ) -> i64 {
        let id = self.id();
        let stem = file.rsplit(['/', '\\']).next().unwrap_or(file).to_owned();
        self.objects.push(
            Node::new(
                "Texture",
                vec![id.into(), name(&stem, "Texture").into(), "".into()],
            )
            .child(Node::new("Type", vec!["TextureVideoClip".into()]))
            .child(Node::new("Version", vec![202.into()]))
            .child(Node::new(
                "TextureName",
                vec![name(&stem, "Texture").into()],
            ))
            .child(Node::new("FileName", vec![file.into()]))
            .child(Node::new("RelativeFilename", vec![file.into()])),
        );
        self.connect_prop(id, material, slot);
        if let Some(bytes) = content {
            let video = self.id();
            self.objects.push(
                Node::new(
                    "Video",
                    vec![video.into(), name(&stem, "Video").into(), "Clip".into()],
                )
                .child(Node::new("Type", vec!["Clip".into()]))
                .child(Node::new("FileName", vec![file.into()]))
                .child(Node::new("RelativeFilename", vec![file.into()]))
                .child(Node::new("Content", vec![Prop::Raw(bytes)])),
            );
            self.connect(video, id);
        }
        id
    }

    /// A skin on `geometry`; clusters: (bone model, vertex indices, weights, bone world matrix
    /// at bind, mesh world matrix at bind), matrices column-major.
    pub fn skin(&mut self, geometry: i64, clusters: &[Cluster]) {
        let skin = self.id();
        self.objects.push(
            Node::new(
                "Deformer",
                vec![skin.into(), name("Skin", "Deformer").into(), "Skin".into()],
            )
            .child(Node::new("Version", vec![101.into()]))
            .child(Node::new("Link_DeformAcuracy", vec![50.0.into()])),
        );
        self.connect(skin, geometry);
        for (bone, indexes, weights, bone_world, mesh_world) in clusters {
            let id = self.id();
            let transform = bone_world.inverse() * *mesh_world;
            self.objects.push(
                Node::new(
                    "Deformer",
                    vec![
                        id.into(),
                        name("Cluster", "SubDeformer").into(),
                        "Cluster".into(),
                    ],
                )
                .child(Node::new("Version", vec![100.into()]))
                .child(Node::new("Indexes", vec![Prop::I32s(indexes.clone())]))
                .child(Node::new("Weights", vec![Prop::F64s(weights.clone())]))
                .child(Node::new(
                    "Transform",
                    vec![Prop::F64s(transform.to_cols_array().to_vec())],
                ))
                .child(Node::new(
                    "TransformLink",
                    vec![Prop::F64s(bone_world.to_cols_array().to_vec())],
                )),
            );
            self.connect(id, skin);
            self.connect(*bone, id);
        }
    }

    /// A blend shape deformer on `geometry` with one channel per shape (name, vertex indices,
    /// position offsets). Returns the channel ids.
    pub fn blend_shapes(&mut self, geometry: i64, shapes: &[Shape]) -> Vec<i64> {
        let deformer = self.id();
        self.objects.push(
            Node::new(
                "Deformer",
                vec![
                    deformer.into(),
                    name("Shapes", "Deformer").into(),
                    "BlendShape".into(),
                ],
            )
            .child(Node::new("Version", vec![100.into()])),
        );
        self.connect(deformer, geometry);
        let mut channels = Vec::new();
        for (n, indexes, offsets) in shapes {
            let channel = self.id();
            self.objects.push(
                Node::new(
                    "Deformer",
                    vec![
                        channel.into(),
                        name(n, "SubDeformer").into(),
                        "BlendShapeChannel".into(),
                    ],
                )
                .child(Node::new("Version", vec![100.into()]))
                .child(Node::new("DeformPercent", vec![0.0.into()]))
                .child(Node::new("FullWeights", vec![Prop::F64s(vec![100.0])])),
            );
            self.connect(channel, deformer);
            let shape = self.id();
            self.objects.push(
                Node::new(
                    "Geometry",
                    vec![shape.into(), name(n, "Geometry").into(), "Shape".into()],
                )
                .child(Node::new("Version", vec![100.into()]))
                .child(Node::new("Indexes", vec![Prop::I32s(indexes.clone())]))
                .child(Node::new(
                    "Vertices",
                    vec![Prop::F64s(offsets.iter().flatten().copied().collect())],
                ))
                .child(Node::new(
                    "Normals",
                    vec![Prop::F64s(vec![0.0; offsets.len() * 3])],
                )),
            );
            self.connect(shape, channel);
            channels.push(channel);
        }
        channels
    }

    /// An animation stack with one layer; returns the layer id.
    pub fn anim_stack(&mut self, n: &str, seconds: f64) -> i64 {
        let stack = self.id();
        let stop = (seconds * KTIME as f64).round() as i64;
        self.stop_seconds = self.stop_seconds.max(seconds);
        self.objects.push(
            Node::new(
                "AnimationStack",
                vec![stack.into(), name(n, "AnimStack").into(), "".into()],
            )
            .child(props70(vec![
                p("LocalStart", "KTime", "Time", vec![0i64.into()]),
                p("LocalStop", "KTime", "Time", vec![stop.into()]),
                p("ReferenceStart", "KTime", "Time", vec![0i64.into()]),
                p("ReferenceStop", "KTime", "Time", vec![stop.into()]),
            ])),
        );
        let layer = self.id();
        self.objects.push(Node::new(
            "AnimationLayer",
            vec![
                layer.into(),
                name("BaseLayer", "AnimLayer").into(),
                "".into(),
            ],
        ));
        self.connect(layer, stack);
        layer
    }

    /// A curve node driving property `prop` ("Lcl Rotation", "DeformPercent") of `target`,
    /// with linear curves per channel ("d|X", …): (channel, times in seconds, values).
    pub fn anim_curves(
        &mut self,
        layer: i64,
        target: i64,
        prop: &str,
        channels: &[(&str, Vec<f64>, Vec<f32>)],
    ) {
        let node = self.id();
        let short = match prop {
            "Lcl Translation" => "T",
            "Lcl Rotation" => "R",
            "Lcl Scaling" => "S",
            other => other,
        };
        self.objects.push(
            Node::new(
                "AnimationCurveNode",
                vec![node.into(), name(short, "AnimCurveNode").into(), "".into()],
            )
            .child(props70(
                channels
                    .iter()
                    .map(|(c, _, v)| p(c, "Number", "", vec![(v[0] as f64).into()]))
                    .collect(),
            )),
        );
        self.connect(node, layer);
        self.connect_prop(node, target, prop);
        for (c, times, values) in channels {
            let curve = self.id();
            let n = times.len() as i32;
            self.objects.push(
                Node::new(
                    "AnimationCurve",
                    vec![curve.into(), name("", "AnimCurve").into(), "".into()],
                )
                .child(Node::new("Default", vec![(values[0] as f64).into()]))
                .child(Node::new("KeyVer", vec![4009.into()]))
                .child(Node::new(
                    "KeyTime",
                    vec![Prop::I64s(
                        times
                            .iter()
                            .map(|t| (t * KTIME as f64).round() as i64)
                            .collect(),
                    )],
                ))
                .child(Node::new("KeyValueFloat", vec![Prop::F32s(values.clone())]))
                // One attribute shared by every key: linear interpolation.
                .child(Node::new("KeyAttrFlags", vec![Prop::I32s(vec![4])]))
                .child(Node::new(
                    "KeyAttrDataFloat",
                    vec![Prop::F32s(vec![0.0; 4])],
                ))
                .child(Node::new("KeyAttrRefCount", vec![Prop::I32s(vec![n])])),
            );
            self.connect_prop(curve, node, c);
        }
    }

    /// The file's bytes.
    pub fn finish(self, axes: Axes) -> Vec<u8> {
        let (up, front_sign) = if axes.up == 2 { (2, -1) } else { (1, 1) };
        let front = if axes.up == 2 { 1 } else { 2 };
        let stop = (self.stop_seconds * KTIME as f64).round() as i64;
        let header = Node::new("FBXHeaderExtension", Vec::new())
            .child(Node::new("FBXHeaderVersion", vec![1003.into()]))
            .child(Node::new("FBXVersion", vec![7400.into()]))
            .child(Node::new(
                "Creator",
                vec!["Misty Engine test writer".into()],
            ));
        let settings = Node::new("GlobalSettings", Vec::new())
            .child(Node::new("Version", vec![1000.into()]))
            .child(props70(vec![
                p("UpAxis", "int", "Integer", vec![up.into()]),
                p("UpAxisSign", "int", "Integer", vec![1.into()]),
                p("FrontAxis", "int", "Integer", vec![front.into()]),
                p("FrontAxisSign", "int", "Integer", vec![front_sign.into()]),
                p("CoordAxis", "int", "Integer", vec![0.into()]),
                p("CoordAxisSign", "int", "Integer", vec![1.into()]),
                p("OriginalUpAxis", "int", "Integer", vec![up.into()]),
                p("OriginalUpAxisSign", "int", "Integer", vec![1.into()]),
                p(
                    "UnitScaleFactor",
                    "double",
                    "Number",
                    vec![axes.unit_scale.into()],
                ),
                p(
                    "OriginalUnitScaleFactor",
                    "double",
                    "Number",
                    vec![axes.unit_scale.into()],
                ),
                p("TimeMode", "enum", "", vec![6.into()]),
                p("TimeSpanStart", "KTime", "Time", vec![0i64.into()]),
                p("TimeSpanStop", "KTime", "Time", vec![stop.into()]),
                p("CustomFrameRate", "double", "Number", vec![30.0.into()]),
            ]));
        let objects = Node {
            name: "Objects".into(),
            props: Vec::new(),
            children: self.objects,
        };
        let connections = Node {
            name: "Connections".into(),
            props: Vec::new(),
            children: self.connections,
        };
        let mut out = Vec::new();
        out.extend_from_slice(b"Kaydara FBX Binary  \0\x1a\0");
        out.extend_from_slice(&7400u32.to_le_bytes());
        for n in [header, settings, objects, connections] {
            n.write(&mut out);
        }
        out.extend_from_slice(&[0u8; 13]);
        out
    }
}
