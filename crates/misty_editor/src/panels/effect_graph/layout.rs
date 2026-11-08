//! Where everything sits on the canvas, in graph units: contexts (header, settings rows, block
//! stack, add-block footer), blocks (title, settings row, one row per input) and operators. The
//! drawing code and hit-testing both read these.

use egui::{Pos2, Rect, pos2, vec2};
use misty_engine::vfx::graph::{Context, ContextKind, EffectGraph, Operator};
use misty_engine::vfx::types::{Port, ValueType};
use misty_engine::vfx::{BlockKind, ContextType};

pub const CTX_W: f32 = 300.0;
pub const HEADER_H: f32 = 34.0;
pub const SETTING_H: f32 = 24.0;
pub const TITLE_H: f32 = 24.0;
pub const ROW_H: f32 = 22.0;
pub const GAP: f32 = 5.0;
pub const PAD: f32 = 8.0;
pub const FOOT_H: f32 = 26.0;
pub const OP_W: f32 = 210.0;
pub const OP_HEADER: f32 = 26.0;

pub struct InputRow {
    pub port: Port,
    /// The row (label and inline value).
    pub rect: Rect,
    /// Port circle centre.
    pub at: Pos2,
}

pub struct BlockLayout {
    pub id: u32,
    pub rect: Rect,
    pub title: Rect,
    pub settings: Option<Rect>,
    pub inputs: Vec<InputRow>,
}

pub struct ContextLayout {
    pub id: u32,
    pub rect: Rect,
    pub header: Rect,
    pub settings: Vec<Rect>,
    pub blocks: Vec<BlockLayout>,
    pub foot: Rect,
}

pub struct OperatorLayout {
    pub id: u32,
    pub rect: Rect,
    pub header: Rect,
    pub settings: Option<Rect>,
    pub inputs: Vec<InputRow>,
    pub outputs: Vec<(Port, Pos2)>,
}

/// Settings rows a context shows under its header.
pub fn setting_rows(kind: &ContextKind) -> usize {
    match kind {
        ContextKind::Spawn => 0,
        ContextKind::GpuEvent(_) => 1,
        ContextKind::Initialize(_) => 4,
        ContextKind::Update(_) => 1,
        ContextKind::Output(_) => 4,
    }
}

/// Blocks with a settings row (attribute, mode, …).
pub fn has_settings(kind: &BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::SetAttribute { .. }
            | BlockKind::PositionSphere { .. }
            | BlockKind::PositionBox { .. }
            | BlockKind::PositionCone { .. }
            | BlockKind::PositionCircle { .. }
            | BlockKind::CollideSphere { .. }
            | BlockKind::Orient { .. }
    )
}

/// Operators with a settings row.
pub fn op_has_settings(kind: &misty_engine::vfx::OperatorKind) -> bool {
    use misty_engine::vfx::OperatorKind as K;
    matches!(
        kind,
        K::GetAttribute { .. } | K::Random { .. } | K::SubgraphOutput { .. }
    )
}

pub fn context(c: &Context) -> ContextLayout {
    let x = c.position[0];
    let mut y = c.position[1];
    let header = Rect::from_min_size(pos2(x, y), vec2(CTX_W, HEADER_H));
    y += HEADER_H;
    let mut settings = Vec::new();
    for _ in 0..setting_rows(&c.kind) {
        settings.push(Rect::from_min_size(
            pos2(x + PAD, y + 2.0),
            vec2(CTX_W - 2.0 * PAD, SETTING_H - 2.0),
        ));
        y += SETTING_H;
    }
    if !settings.is_empty() {
        y += 4.0;
    }
    y += GAP;
    let mut blocks = Vec::new();
    for b in &c.blocks {
        let top = y;
        let title = Rect::from_min_size(pos2(x + PAD, y), vec2(CTX_W - 2.0 * PAD, TITLE_H));
        y += TITLE_H;
        let settings_row = has_settings(&b.kind).then(|| {
            let r = Rect::from_min_size(
                pos2(x + PAD + 6.0, y),
                vec2(CTX_W - 2.0 * PAD - 12.0, ROW_H),
            );
            y += ROW_H;
            r
        });
        let mut inputs = Vec::new();
        for p in b.kind.inputs() {
            let rect = Rect::from_min_size(
                pos2(x + PAD + 6.0, y),
                vec2(CTX_W - 2.0 * PAD - 12.0, ROW_H),
            );
            inputs.push(InputRow {
                port: p,
                rect,
                at: pos2(x, y + ROW_H * 0.5),
            });
            y += ROW_H;
        }
        y += 4.0;
        blocks.push(BlockLayout {
            id: b.id,
            rect: Rect::from_min_max(pos2(x + PAD, top), pos2(x + CTX_W - PAD, y)),
            title,
            settings: settings_row,
            inputs,
        });
        y += GAP;
    }
    let foot = Rect::from_min_size(pos2(x + PAD, y), vec2(CTX_W - 2.0 * PAD, FOOT_H - 4.0));
    y += FOOT_H + 2.0;
    ContextLayout {
        id: c.id,
        rect: Rect::from_min_max(pos2(x, c.position[1]), pos2(x + CTX_W, y)),
        header,
        settings,
        blocks,
        foot,
    }
}

pub fn operator(g: &EffectGraph, o: &Operator) -> OperatorLayout {
    let (x, y0) = (o.position[0], o.position[1]);
    let header = Rect::from_min_size(pos2(x, y0), vec2(OP_W, OP_HEADER));
    let mut y = y0 + OP_HEADER + 4.0;
    let settings = op_has_settings(&o.kind).then(|| {
        let r = Rect::from_min_size(pos2(x + 10.0, y), vec2(OP_W - 20.0, ROW_H));
        y += ROW_H;
        r
    });
    let outs = g.outputs(o);
    let ins = o.kind.inputs();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let rows = ins.len().max(outs.len());
    for i in 0..rows {
        if let Some(p) = ins.get(i) {
            inputs.push(InputRow {
                port: p.clone(),
                rect: Rect::from_min_size(pos2(x + 12.0, y), vec2(OP_W - 24.0, ROW_H)),
                at: pos2(x, y + ROW_H * 0.5),
            });
        }
        if let Some(p) = outs.get(i) {
            outputs.push((p.clone(), pos2(x + OP_W, y + ROW_H * 0.5)));
        }
        y += ROW_H;
    }
    y += 6.0;
    OperatorLayout {
        id: o.id,
        rect: Rect::from_min_max(pos2(x, y0), pos2(x + OP_W, y.max(y0 + OP_HEADER + 12.0))),
        header,
        settings,
        inputs,
        outputs,
    }
}

/// Every input port: (owner id, port name, graph position, type).
pub fn input_ports(g: &EffectGraph) -> Vec<(u32, &'static str, Pos2, ValueType)> {
    let types = g.types();
    let mut out = Vec::new();
    for c in &g.contexts {
        for b in context(c).blocks {
            for r in b.inputs {
                out.push((b.id, r.port.name, r.at, r.port.ty.resolve(ValueType::Float)));
            }
        }
    }
    for o in &g.operators {
        let d = types.dynamic(o.id);
        for r in operator(g, o).inputs {
            out.push((o.id, r.port.name, r.at, r.port.ty.resolve(d)));
        }
    }
    out
}

/// Every operator output: (operator id, port name, graph position, type).
pub fn output_ports(g: &EffectGraph) -> Vec<(u32, &'static str, Pos2, ValueType)> {
    let types = g.types();
    let mut out = Vec::new();
    for o in &g.operators {
        for (p, at) in operator(g, o).outputs {
            let ty = types.output(o.id, p.name).unwrap_or(ValueType::Float);
            out.push((o.id, p.name, at, ty));
        }
    }
    out
}

/// Flow order of the contexts (Spawn / GPU Event → Initialize → Update → Outputs, per system,
/// and parent → GPU Event): pairs to connect.
pub fn flow(g: &EffectGraph) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for sys in g.systems() {
        let first = |t| g.system_context(sys, t).map(|c| c.id);
        let start = first(ContextType::Spawn).or(first(ContextType::GpuEvent));
        let init = first(ContextType::Initialize);
        let update = first(ContextType::Update);
        if let (Some(a), Some(b)) = (start, init) {
            out.push((a, b));
        }
        let before_out = update.or(init);
        if let (Some(a), Some(b)) = (init, update) {
            out.push((a, b));
        }
        if let Some(a) = before_out {
            for c in g
                .contexts
                .iter()
                .filter(|c| c.system == sys && c.kind.ty() == ContextType::Output)
            {
                out.push((a, c.id));
            }
        }
        // The parent system's Update (or Initialize) sends the events.
        if let Some(ev) = g.system_context(sys, ContextType::GpuEvent)
            && let ContextKind::GpuEvent(s) = &ev.kind
        {
            let from = g
                .system_context(s.parent, ContextType::Update)
                .or(g.system_context(s.parent, ContextType::Initialize));
            if let Some(f) = from {
                out.push((f.id, ev.id));
            }
        }
    }
    out
}

/// Bounds of everything (graph units).
pub fn bounds(g: &EffectGraph) -> Rect {
    let mut r = Rect::NOTHING;
    for c in &g.contexts {
        r = r.union(context(c).rect);
    }
    for o in &g.operators {
        r = r.union(operator(g, o).rect);
    }
    r
}

/// Tidy layout: Spawn, Initialize and Update stacked, Outputs side by side below them, each
/// operator left of the input it feeds (operators feeding operators further left).
pub fn arrange(g: &mut EffectGraph) {
    let gap = 60.0;
    // Systems side by side: each a column of contexts with its outputs in a row below.
    let mut x0 = 0.0;
    for sys in g.systems() {
        let mut y = 0.0;
        let mut right: f32 = x0 + CTX_W;
        for t in [
            ContextType::Spawn,
            ContextType::GpuEvent,
            ContextType::Initialize,
            ContextType::Update,
        ] {
            let ids: Vec<u32> = g
                .contexts
                .iter()
                .filter(|c| c.system == sys && c.kind.ty() == t)
                .map(|c| c.id)
                .collect();
            for id in ids {
                let c = g.context_mut(id).expect("listed");
                c.position = [x0, y];
                y = context(c).rect.bottom() + gap;
            }
        }
        let outs: Vec<u32> = g
            .contexts
            .iter()
            .filter(|c| c.system == sys && c.kind.ty() == ContextType::Output)
            .map(|c| c.id)
            .collect();
        for (i, id) in outs.into_iter().enumerate() {
            let x = x0 + i as f32 * (CTX_W + 50.0);
            g.context_mut(id).expect("listed").position = [x, y];
            right = right.max(x + CTX_W);
        }
        x0 = right + 120.0;
    }
    // Consumers before producers: reverse dependency order.
    let mut order = g.operator_order();
    order.reverse();
    let ports = |g: &EffectGraph| input_ports(g);
    let mut used: Vec<Rect> = Vec::new();
    for id in order {
        let Some(link) = g.links.iter().find(|l| l.from == id).cloned() else {
            continue;
        };
        let Some((_, _, at, _)) = ports(g)
            .into_iter()
            .find(|(n, p, _, _)| *n == link.to && *p == link.input)
        else {
            continue;
        };
        let Some(o) = g.operator(id) else { continue };
        let h = operator(g, o).rect.height();
        let mut r = Rect::from_min_size(pos2(at.x - OP_W - 50.0, at.y - 14.0), vec2(OP_W, h));
        while used.iter().any(|u| u.expand(6.0).intersects(r)) {
            r = r.translate(vec2(0.0, 30.0));
        }
        used.push(r);
        if let Some(o) = g.operator_mut(id) {
            o.position = [r.left(), r.top()];
        }
    }
}
