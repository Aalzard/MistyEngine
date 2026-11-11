//! The bytecode interpreter. One call runs a flow until it ends, needs Lua (the runtime makes the
//! call and resumes), fails, or stops at a breakpoint (the flow is kept for stepping).

use std::collections::HashSet;
use std::rc::Rc;

use hecs::Entity;

use super::compile::{NO_SLOT, Op, Program};
use super::natives::Cx;
use super::value::{PinType, Value};
use crate::scripting::World;

/// Most ops one flow may run (catches exec wires that loop forever).
pub const OP_BUDGET: u64 = 20_000_000;
/// Deepest nesting of Sequence / loop bodies.
pub const MAX_DEPTH: usize = 512;

/// A running flow: where it is and where its branches return to.
#[derive(Clone, Debug, Default)]
pub struct Flow {
    pub pc: u32,
    pub stack: Vec<u32>,
    pub ops: u64,
}

impl Flow {
    pub fn at(pc: u32) -> Self {
        Self {
            pc,
            stack: Vec::new(),
            ops: 0,
        }
    }
}

/// A Delay waiting to resume.
#[derive(Clone, Debug)]
pub struct Pending {
    /// Game time to resume at.
    pub at: f64,
    pub pc: u32,
}

/// One running graph's memory.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub regs: Vec<Value>,
    pub delays: Vec<Pending>,
}

impl State {
    pub fn new(prog: &Program, this: Entity) -> Self {
        let mut regs = prog.init.clone();
        regs[0] = Value::Object(this);
        Self {
            regs,
            delays: Vec::new(),
        }
    }
}

/// Why a run stopped.
#[derive(Debug)]
pub enum Exit {
    Done,
    /// Make Lua call `i` of the program, then run again.
    Lua(u32),
    /// A node failed.
    Error {
        node: u32,
        message: String,
    },
    /// Stopped before a node (breakpoint or step).
    Break {
        node: u32,
    },
}

/// Breakpoints and stepping for instances of the graph being debugged.
pub struct Hook<'a> {
    pub breakpoints: &'a HashSet<u32>,
    /// Stop at the next node, whatever it is.
    pub step: bool,
    /// Don't stop at this address (resuming from it).
    pub skip: Option<u32>,
    /// Entries reached (node, pin), for the active wires; None = not recorded.
    pub hits: Option<&'a mut Vec<(u32, Rc<str>)>>,
}

/// A value fitted to a variable's type.
fn fit(v: &Value, ty: PinType) -> Value {
    match (ty, v) {
        (PinType::Float, Value::Float(_))
        | (PinType::Int, Value::Int(_))
        | (PinType::Bool, Value::Bool(_))
        | (PinType::Vector3, Value::Vec3(_))
        | (PinType::Quaternion, Value::Quat(_))
        | (PinType::String, Value::Str(_)) => v.clone(),
        (PinType::Any | PinType::GameObject | PinType::List | PinType::Exec, _) => v.clone(),
        _ => v.convert(ty),
    }
}

/// Run `flow` of `prog`.
#[allow(clippy::too_many_arguments)]
pub fn run(
    prog: &Program,
    st: &mut State,
    obj_index: &[u32],
    world: &mut World,
    this: Entity,
    rng: &mut u64,
    flow: &mut Flow,
    hook: Option<Hook<'_>>,
) -> Exit {
    // Without a debugger the loop has no per-op checks at all.
    if hook.is_some() {
        run_impl::<true>(prog, st, obj_index, world, this, rng, flow, hook)
    } else {
        run_impl::<false>(prog, st, obj_index, world, this, rng, flow, None)
    }
}

#[allow(clippy::too_many_arguments)]
fn run_impl<const DEBUG: bool>(
    prog: &Program,
    st: &mut State,
    obj_index: &[u32],
    world: &mut World,
    this: Entity,
    rng: &mut u64,
    flow: &mut Flow,
    mut hook: Option<Hook<'_>>,
) -> Exit {
    let code = &prog.code;
    let mut pc = flow.pc;
    let mut ops = flow.ops;
    macro_rules! ret {
        () => {
            match flow.stack.pop() {
                Some(r) => pc = r,
                None => {
                    flow.pc = pc;
                    flow.ops = ops;
                    return Exit::Done;
                }
            }
        };
    }
    loop {
        if DEBUG && let Some(h) = hook.as_mut() {
            let marks = &prog.marks[pc as usize];
            if !marks.is_empty() {
                let skip = h.skip == Some(pc);
                if skip {
                    h.skip = None;
                } else {
                    let hit = marks.iter().find(|(n, _)| h.breakpoints.contains(n));
                    if h.step || hit.is_some() {
                        flow.pc = pc;
                        flow.ops = ops;
                        let node = hit.unwrap_or(&marks[0]).0;
                        return Exit::Break { node };
                    }
                }
                if let Some(hits) = h.hits.as_deref_mut() {
                    hits.extend(marks.iter().cloned());
                }
            }
        }
        ops += 1;
        if ops > OP_BUDGET {
            flow.pc = pc;
            flow.ops = ops;
            let node = (0..=pc as usize)
                .rev()
                .find_map(|p| prog.marks.get(p).and_then(|m| m.first()).map(|m| m.0))
                .unwrap_or(0);
            return Exit::Error {
                node,
                message: format!(
                    "the flow ran more than {OP_BUDGET} steps without ending (an exec loop?)"
                ),
            };
        }
        let op = code[pc as usize];
        pc += 1;
        match op {
            Op::Native {
                f,
                at,
                ins,
                outs,
                node,
            } => {
                let a = at as usize;
                let b = a + ins as usize;
                let c = b + outs as usize;
                let mut cx = Cx {
                    regs: &mut st.regs,
                    world,
                    this,
                    rng,
                };
                if let Err(message) = f(&mut cx, &prog.pool[a..b], &prog.pool[b..c]) {
                    flow.pc = pc;
                    flow.ops = ops;
                    return Exit::Error { node, message };
                }
            }
            Op::Jump(t) => pc = t,
            Op::JumpIf { cond, to } => {
                if st.regs[cond as usize].as_bool() {
                    pc = to;
                }
            }
            Op::JumpIfNot { cond, to } => {
                if !st.regs[cond as usize].as_bool() {
                    pc = to;
                }
            }
            Op::Call(t) => {
                if flow.stack.len() >= MAX_DEPTH {
                    flow.pc = pc;
                    return Exit::Error {
                        node: 0,
                        message: "flow nodes nested too deeply".into(),
                    };
                }
                flow.stack.push(pc);
                pc = t;
            }
            Op::Ret => ret!(),
            Op::Copy { from, to } => {
                st.regs[to as usize] = st.regs[from as usize].clone();
            }
            Op::SetBool { slot, value } => st.regs[slot as usize] = Value::Bool(value),
            Op::Toggle { slot } => {
                let v = !st.regs[slot as usize].as_bool();
                st.regs[slot as usize] = Value::Bool(v);
            }
            Op::ForInit {
                first,
                last,
                index,
                end,
            } => {
                st.regs[index as usize] = Value::Int(st.regs[first as usize].as_int());
                st.regs[end as usize] = Value::Int(st.regs[last as usize].as_int());
            }
            Op::ForTest { index, end, exit } => {
                if st.regs[index as usize].as_int() > st.regs[end as usize].as_int() {
                    pc = exit;
                }
            }
            Op::Incr { slot } => {
                // Saturating: a For loop up to i64::MAX overflowed here (the op budget still
                // stops a loop that never ends).
                let v = st.regs[slot as usize].as_int().saturating_add(1);
                st.regs[slot as usize] = Value::Int(v);
            }
            Op::ForEachInit {
                list,
                copy,
                index,
                node,
            } => {
                let l = &st.regs[list as usize];
                if !matches!(l, Value::List(_) | Value::Nil) {
                    flow.pc = pc;
                    return Exit::Error {
                        node,
                        message: format!("For Each needs a List, got {}", l.type_name()),
                    };
                }
                st.regs[copy as usize] = l.clone();
                st.regs[index as usize] = Value::Int(0);
            }
            Op::ForEachTest {
                copy,
                index,
                item,
                exit,
            } => {
                let i = st.regs[index as usize].as_int();
                let next = match &st.regs[copy as usize] {
                    Value::List(l) => usize::try_from(i).ok().and_then(|i| l.get(i).cloned()),
                    _ => None,
                };
                match next {
                    Some(v) => st.regs[item as usize] = v,
                    None => pc = exit,
                }
            }
            Op::GuardInit { counter } => st.regs[counter as usize] = Value::Int(0),
            Op::Guard { counter, max, node } => {
                let n = st.regs[counter as usize].as_int() + 1;
                st.regs[counter as usize] = Value::Int(n);
                let limit = st.regs[max as usize].as_int().max(1);
                if n > limit {
                    flow.pc = pc;
                    return Exit::Error {
                        node,
                        message: format!(
                            "While looped {limit} times without its condition turning false \
                             (raise Max Iterations if that is meant)"
                        ),
                    };
                }
            }
            Op::FlipFlop { state, is_a, b } => {
                let a = !st.regs[state as usize].as_bool();
                st.regs[state as usize] = Value::Bool(a);
                st.regs[is_a as usize] = Value::Bool(a);
                if !a {
                    pc = b;
                }
            }
            Op::Delay {
                pending,
                duration,
                resume,
            } => {
                if !st.regs[pending as usize].as_bool() {
                    st.regs[pending as usize] = Value::Bool(true);
                    let d = f64::from(st.regs[duration as usize].as_float().max(0.0));
                    st.delays.push(Pending {
                        at: world.time.time + d,
                        pc: resume,
                    });
                }
                ret!();
            }
            Op::SetVar { var, from, out } => {
                let info = &prog.vars[var as usize];
                let v = fit(&st.regs[from as usize], info.ty);
                if out != NO_SLOT {
                    st.regs[out as usize] = v.clone();
                }
                st.regs[info.slot as usize] = v;
            }
            Op::GetObjVar { var, out } => {
                let i = obj_index[var as usize];
                st.regs[out as usize] = world.graph_vars.at(this, i as usize);
            }
            Op::SetObjVar { var, from, out } => {
                let i = obj_index[var as usize];
                let v = fit(&st.regs[from as usize], prog.object_vars[var as usize].ty);
                if out != NO_SLOT {
                    st.regs[out as usize] = v.clone();
                }
                world.graph_vars.set_at(this, i as usize, v);
            }
            Op::GetOtherVar {
                target,
                name,
                out,
                node,
            } => {
                let Some(e) = st.regs[target as usize].as_object() else {
                    flow.pc = pc;
                    return Exit::Error {
                        node,
                        message: "Get Object Variable needs a GameObject".into(),
                    };
                };
                let name = st.regs[name as usize].to_text(None);
                st.regs[out as usize] = world.graph_vars.get(e, &name).unwrap_or(Value::Nil);
            }
            Op::SetOtherVar {
                target,
                name,
                from,
                node,
            } => {
                let Some(e) = st.regs[target as usize].as_object() else {
                    flow.pc = pc;
                    return Exit::Error {
                        node,
                        message: "Set Object Variable needs a GameObject".into(),
                    };
                };
                let name = st.regs[name as usize].to_text(None);
                let v = st.regs[from as usize].clone();
                world.graph_vars.set(e, &name, v);
            }
            Op::F1 { f, a, out } => {
                let r = f.eval(st.regs[a as usize].as_float());
                st.regs[out as usize] = Value::Float(r);
            }
            Op::F2 { f, a, b, out } => {
                let r = f.eval(
                    st.regs[a as usize].as_float(),
                    st.regs[b as usize].as_float(),
                );
                st.regs[out as usize] = Value::Float(r);
            }
            Op::F3 { f, a, b, c, out } => {
                let r = f.eval(
                    st.regs[a as usize].as_float(),
                    st.regs[b as usize].as_float(),
                    st.regs[c as usize].as_float(),
                );
                st.regs[out as usize] = Value::Float(r);
            }
            Op::Lua(i) => {
                flow.pc = pc;
                flow.ops = ops;
                return Exit::Lua(i);
            }
            Op::Send(i) => {
                let s = &prog.sends[i as usize];
                let target = match s.target {
                    Some(t) => match st.regs[t as usize].as_object() {
                        Some(e) => Some(e),
                        None => {
                            flow.pc = pc;
                            return Exit::Error {
                                node: 0,
                                message: format!("Send Custom Event '{}' needs a target", s.name),
                            };
                        }
                    },
                    None => None,
                };
                world.graph_events.push(super::runtime::CustomEvent {
                    target,
                    name: s.name.clone(),
                    args: s
                        .args
                        .iter()
                        .map(|a| st.regs[*a as usize].clone())
                        .collect(),
                });
            }
        }
    }
}
