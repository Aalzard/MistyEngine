//! Node preview thumbnails: the graph evaluated on the CPU (`shader_graph::eval`) into small
//! egui textures, redone when the graph's content changes (and ten times a second while it reads
//! Time). Nodes reading 3D geometry show a sphere, the others a flat UV square.

use std::cell::RefCell;
use std::collections::HashMap;

use misty_engine::shader_graph::eval::{self, Context, Textures};
use misty_engine::shader_graph::{
    GraphProperty, NodeKind, PropertyType, ShaderGraph, TextureFallback,
};

/// Preview size in pixels.
const SIZE: usize = 64;

#[derive(Default)]
pub struct NodePreviews {
    textures: HashMap<u32, egui::TextureHandle>,
    dirty: bool,
    last: f64,
    images: ImageCache,
}

impl NodePreviews {
    pub fn invalidate(&mut self) {
        self.dirty = true;
        // A saved Sub Graph shows on the next refresh.
        self.images.graphs.borrow_mut().clear();
    }

    pub fn texture(&self, node: u32) -> Option<egui::TextureId> {
        self.textures.get(&node).map(|t| t.id())
    }

    pub fn update(&mut self, ctx: &egui::Context, g: &ShaderGraph) {
        if self.dirty {
            self.dirty = false;
            self.render(ctx, g);
        }
    }

    /// Re-render animated graphs a few times a second.
    pub fn animate(&mut self, ctx: &egui::Context, g: &ShaderGraph) {
        if !eval::uses_time(g) {
            return;
        }
        let now = ctx.input(|i| i.time);
        if now - self.last > 0.1 {
            self.render(ctx, g);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    fn render(&mut self, ctx: &egui::Context, g: &ShaderGraph) {
        self.last = ctx.input(|i| i.time);
        let time = self.last as f32;
        let flat = eval::evaluate(
            g,
            &Context {
                size: SIZE,
                time,
                sphere: false,
            },
            &self.images,
        );
        let sphere_nodes: Vec<u32> = g
            .nodes
            .iter()
            .filter(|n| eval::wants_sphere(g, n.id))
            .map(|n| n.id)
            .collect();
        let round = (!sphere_nodes.is_empty()).then(|| {
            eval::evaluate(
                g,
                &Context {
                    size: SIZE,
                    time,
                    sphere: true,
                },
                &self.images,
            )
        });
        let mut keep = Vec::new();
        for n in &g.nodes {
            if n.kind == NodeKind::Master || n.hide_preview || !super::canvas::has_preview(&n.kind)
            {
                continue;
            }
            let Some(out) = g.outputs(n).first().map(|p| p.name) else {
                continue;
            };
            let source = match &round {
                Some(r) if sphere_nodes.contains(&n.id) => r,
                _ => &flat,
            };
            let Some(pixels) = source.image(n.id, out) else {
                continue;
            };
            let bytes: Vec<u8> = pixels.into_iter().flatten().collect();
            let image = egui::ColorImage::from_rgba_unmultiplied([SIZE, SIZE], &bytes);
            let options = egui::TextureOptions::LINEAR;
            match self.textures.get_mut(&n.id) {
                Some(t) => t.set(image, options),
                None => {
                    let t = ctx.load_texture(format!("sg_preview_{}", n.id), image, options);
                    self.textures.insert(n.id, t);
                }
            }
            keep.push(n.id);
        }
        self.textures.retain(|id, _| keep.contains(id));
    }
}

/// A decoded image: width, height and linear pixels.
type Decoded = Option<(usize, usize, Vec<[f32; 4]>)>;

/// Small linear-RGBA copies of the textures assigned to Texture2D properties.
#[derive(Default)]
struct ImageCache {
    images: RefCell<HashMap<(String, bool), Decoded>>,
    /// Sub Graphs nodes use (read once per window session).
    graphs: RefCell<HashMap<String, Option<std::sync::Arc<ShaderGraph>>>>,
}

fn decode(path: &str, linear: bool) -> Decoded {
    let img = misty_engine::image::open(misty_engine::assets::absolute(path)).ok()?;
    let img = img.thumbnail(128, 128).to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let srgb = |c: u8| {
        let c = c as f32 / 255.0;
        if linear {
            c
        } else if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let px = img
        .pixels()
        .map(|p| [srgb(p[0]), srgb(p[1]), srgb(p[2]), p[3] as f32 / 255.0])
        .collect();
    Some((w, h, px))
}

impl Textures for ImageCache {
    fn graph(&self, path: &str) -> Option<std::sync::Arc<ShaderGraph>> {
        let mut cache = self.graphs.borrow_mut();
        cache
            .entry(path.to_owned())
            .or_insert_with(|| {
                ShaderGraph::load(&misty_engine::assets::absolute(path))
                    .ok()
                    .map(std::sync::Arc::new)
            })
            .clone()
    }

    fn sample(&self, property: &GraphProperty, uv: [f32; 2]) -> [f32; 4] {
        let Some(path) = &property.texture else {
            return eval::fallback_color(property);
        };
        let linear = matches!(
            property.ty,
            PropertyType::Texture2D {
                fallback: TextureFallback::Normal
            }
        );
        let mut cache = self.images.borrow_mut();
        let entry = cache
            .entry((path.clone(), linear))
            .or_insert_with(|| decode(path, linear));
        let Some((w, h, px)) = entry else {
            return eval::fallback_color(property);
        };
        let x = ((uv[0].rem_euclid(1.0)) * *w as f32) as usize % *w;
        // UV v goes up; image rows go down.
        let y = (((1.0 - uv[1]).rem_euclid(1.0)) * *h as f32) as usize % *h;
        px[y * *w + x]
    }
}
