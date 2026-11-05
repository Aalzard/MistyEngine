//! Lua `MeshRenderer` / `Renderer` (from `GetComponent`) and `Material`.
//!
//! `renderer.material` makes a per-renderer copy on first use (only this object changes, like
//! the reference engine's instance materials); `renderer.sharedMaterial` is the asset itself,
//! so changes show on every object using it. Both last until Play mode ends.
//! Materials use the Standard shader's property names: `SetColor("_Color")`,
//! `SetFloat("_Metallic")`, `SetColor("_EmissionColor")`, `EnableKeyword("_EMISSION")`, …

use hecs::Entity;
use mlua::{FromLua, Lua, MetaMethod, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, LTransform, LVec3, with_world};
use super::ui_api::LColor;
use crate::assets::{Material, SmoothnessSource};
use crate::render::{RenderingMode, ShaderKind};
use crate::scene::MeshRenderer;

#[derive(Clone, Copy)]
pub struct LMeshRenderer(pub Entity);

/// Which material a Lua `Material` stands for.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialRef {
    /// A renderer's own copy of its element `1`.
    Instance(Entity, usize),
    /// A material asset (`None` = Default-Material).
    Shared(Option<String>),
}

#[derive(Clone, Debug)]
pub struct LMaterial(pub MaterialRef);

impl FromLua for LMaterial {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.borrow::<LMaterial>()?.clone()),
            other => Err(mlua::Error::runtime(format!(
                "expected Material, got {}",
                other.type_name()
            ))),
        }
    }
}

fn gone() -> mlua::Error {
    mlua::Error::runtime("object was destroyed")
}

/// Current values of a material asset (script changes first, then the file).
fn shared(w: &World, path: Option<&str>) -> Material {
    let Some(p) = path else {
        return Material::default();
    };
    w.scene
        .shared_materials
        .get(p)
        .cloned()
        .or_else(|| Material::load(&w.root.join(p)).ok())
        .unwrap_or_default()
}

fn renderer<R>(w: &World, e: Entity, f: impl FnOnce(&MeshRenderer) -> R) -> Result<R> {
    let r = w.scene.world.get::<&MeshRenderer>(e).map_err(|_| gone())?;
    Ok(f(&r))
}

/// The renderer's copy of element `slot`, made from the asset on first use.
fn ensure_instance(w: &mut World, e: Entity, slot: usize) -> Result<()> {
    let path = renderer(w, e, |r| {
        r.instance(slot)
            .is_none()
            .then(|| r.material_at(slot).map(str::to_owned))
    })?;
    let Some(path) = path else {
        return Ok(());
    };
    let copy = shared(w, path.as_deref());
    let mut r = w
        .scene
        .world
        .get::<&mut MeshRenderer>(e)
        .map_err(|_| gone())?;
    if r.instances.len() <= slot {
        r.instances.resize(slot + 1, None);
    }
    r.instances[slot] = Some(copy);
    Ok(())
}

fn read(w: &World, m: &MaterialRef) -> Result<Material> {
    match m {
        MaterialRef::Instance(e, slot) => renderer(w, *e, |r| {
            r.instance(*slot)
                .cloned()
                .ok_or_else(|| r.material_at(*slot).map(str::to_owned))
        })?
        .or_else(|path| Ok(shared(w, path.as_deref()))),
        MaterialRef::Shared(path) => Ok(shared(w, path.as_deref())),
    }
}

fn write(w: &mut World, m: &MaterialRef, f: impl FnOnce(&mut Material)) -> Result<()> {
    match m {
        MaterialRef::Instance(e, slot) => {
            ensure_instance(w, *e, *slot)?;
            let mut r = w
                .scene
                .world
                .get::<&mut MeshRenderer>(*e)
                .map_err(|_| gone())?;
            if let Some(Some(mat)) = r.instances.get_mut(*slot) {
                f(mat);
            }
            Ok(())
        }
        MaterialRef::Shared(None) => Err(mlua::Error::runtime(
            "Default-Material can't be changed; assign a material first",
        )),
        MaterialRef::Shared(Some(path)) => {
            let mut mat = shared(w, Some(path));
            f(&mut mat);
            w.scene.shared_materials.insert(path.clone(), mat);
            Ok(())
        }
    }
}

/// Asset name, "Name (Instance)" for a renderer's copy.
fn name(w: &World, m: &MaterialRef) -> String {
    match m {
        MaterialRef::Shared(p) => crate::assets::material_name(p.as_deref()),
        MaterialRef::Instance(e, slot) => {
            let path = renderer(w, *e, |r| r.material_at(*slot).map(str::to_owned))
                .ok()
                .flatten();
            format!(
                "{} (Instance)",
                crate::assets::material_name(path.as_deref())
            )
        }
    }
}

fn get<R>(lua: &Lua, m: &MaterialRef, f: impl FnOnce(&Material) -> R) -> Result<R> {
    with_world(lua, |w| read(w, m).map(|mat| f(&mat)))?
}

fn set(lua: &Lua, m: &MaterialRef, f: impl FnOnce(&mut Material)) -> Result<()> {
    with_world(lua, |w| write(w, m, f))?
}

// ---- property names ---------------------------------------------------------------------------

/// `_EmissionColor` as one HDR colour: colour × intensity.
fn emission_color(m: &Material) -> [f32; 4] {
    let e = m.emission.map(|c| c * m.emission_intensity);
    [e[0], e[1], e[2], 1.0]
}

/// Split an HDR colour into a 0..1 colour and an intensity.
fn set_emission_color(m: &mut Material, c: [f32; 4]) {
    let peak = c[0].max(c[1]).max(c[2]);
    let intensity = peak.max(1.0);
    m.emission = [c[0], c[1], c[2]].map(|v| v.max(0.0) / intensity);
    m.emission_intensity = intensity;
}

fn get_color(m: &Material, name: &str) -> Option<[f32; 4]> {
    match name {
        "_Color" => Some(m.color),
        "_EmissionColor" => Some(emission_color(m)),
        _ => None,
    }
}

fn set_color(m: &mut Material, name: &str, c: [f32; 4]) {
    match name {
        "_Color" => m.color = c,
        "_EmissionColor" => set_emission_color(m, c),
        _ => {}
    }
}

fn get_float(m: &Material, name: &str) -> Option<f32> {
    Some(match name {
        "_Metallic" => m.metallic,
        "_Glossiness" | "_GlossMapScale" => m.smoothness,
        "_BumpScale" => m.normal_scale,
        "_Cutoff" => m.alpha_cutoff,
        "_OcclusionStrength" => m.occlusion_strength,
        "_Mode" => m.rendering_mode.index() as f32,
        "_SmoothnessTextureChannel" => {
            (m.smoothness_source == SmoothnessSource::AlbedoAlpha) as u8 as f32
        }
        // 0 = Off (both faces), 2 = Back.
        "_Cull" => {
            if m.double_sided {
                0.0
            } else {
                2.0
            }
        }
        _ => return None,
    })
}

fn set_float(m: &mut Material, name: &str, v: f32) {
    match name {
        "_Metallic" => m.metallic = v.clamp(0.0, 1.0),
        "_Glossiness" | "_GlossMapScale" => m.smoothness = v.clamp(0.0, 1.0),
        "_BumpScale" => m.normal_scale = v,
        "_Cutoff" => m.alpha_cutoff = v.clamp(0.0, 1.0),
        "_OcclusionStrength" => m.occlusion_strength = v.clamp(0.0, 1.0),
        "_Mode" => {
            let i = (v.round().max(0.0) as usize).min(3);
            m.rendering_mode = RenderingMode::ALL[i];
        }
        "_SmoothnessTextureChannel" => {
            m.smoothness_source = SmoothnessSource::ALL[(v.round() as usize).min(1)];
        }
        "_Cull" => m.double_sided = v.round() as i32 == 0,
        _ => {}
    }
}

/// Texture slot of a property name.
fn texture_slot<'a>(m: &'a mut Material, name: &str) -> Option<&'a mut Option<String>> {
    Some(match name {
        "_MainTex" => &mut m.albedo_map,
        "_BumpMap" => &mut m.normal_map,
        "_MetallicGlossMap" => &mut m.metallic_map,
        "_OcclusionMap" => &mut m.occlusion_map,
        "_EmissionMap" => &mut m.emission_map,
        _ => return None,
    })
}

fn has_property(m: &Material, name: &str) -> bool {
    get_color(m, name).is_some()
        || get_float(m, name).is_some()
        || texture_slot(&mut m.clone(), name).is_some()
}

fn keyword(m: &Material, k: &str) -> bool {
    match k {
        "_EMISSION" => m.emission_enabled,
        "_NORMALMAP" => m.normal_map.is_some(),
        "_METALLICGLOSSMAP" => m.metallic_map.is_some(),
        "_ALPHATEST_ON" => m.rendering_mode == RenderingMode::Cutout,
        "_ALPHABLEND_ON" => m.rendering_mode == RenderingMode::Fade,
        "_ALPHAPREMULTIPLY_ON" => m.rendering_mode == RenderingMode::Transparent,
        _ => false,
    }
}

/// x, y of a Vector3 (z ignored) or a `{ x = …, y = … }` table.
fn vec2(v: Value, lua: &Lua) -> Result<[f32; 2]> {
    match v {
        Value::Table(t) => Ok([t.get("x")?, t.get("y")?]),
        other => {
            let v = LVec3::from_lua(other, lua)?.0;
            Ok([v.x, v.y])
        }
    }
}

fn vec2_value(v: [f32; 2]) -> LVec3 {
    LVec3(glam::Vec3::new(v[0], v[1], 0.0))
}

impl UserData for LMaterial {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("color", |lua, m| get(lua, &m.0, |mat| LColor(mat.color)));
        f.add_field_method_set("color", |lua, m, c: LColor| {
            set(lua, &m.0, |mat| mat.color = c.0)
        });
        f.add_field_method_get("mainTextureOffset", |lua, m| {
            get(lua, &m.0, |mat| vec2_value(mat.offset))
        });
        f.add_field_method_set("mainTextureOffset", |lua, m, v: Value| {
            let v = vec2(v, lua)?;
            set(lua, &m.0, |mat| mat.offset = v)
        });
        f.add_field_method_get("mainTextureScale", |lua, m| {
            get(lua, &m.0, |mat| vec2_value(mat.tiling))
        });
        f.add_field_method_set("mainTextureScale", |lua, m, v: Value| {
            let v = vec2(v, lua)?;
            set(lua, &m.0, |mat| mat.tiling = v)
        });
        f.add_field_method_get("shader", |lua, m| get(lua, &m.0, |mat| mat.shader.clone()));
        f.add_field_method_set("shader", |lua, m, name: String| {
            let kind = ShaderKind::from_name(&name);
            set(lua, &m.0, |mat| mat.shader = kind.name().to_owned())
        });
        f.add_field_method_get("renderQueue", |lua, m| {
            get(lua, &m.0, |mat| mat.params().mode.render_queue())
        });
        f.add_field_method_get("name", |lua, m| with_world(lua, |w| name(w, &m.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("GetColor", |lua, m, name: String| {
            get(lua, &m.0, |mat| {
                LColor(get_color(mat, &name).unwrap_or([0.0; 4]))
            })
        });
        m.add_method("SetColor", |lua, m, (name, c): (String, LColor)| {
            set(lua, &m.0, |mat| set_color(mat, &name, c.0))
        });
        m.add_method("GetFloat", |lua, m, name: String| {
            get(lua, &m.0, |mat| get_float(mat, &name).unwrap_or(0.0))
        });
        m.add_method("SetFloat", |lua, m, (name, v): (String, f32)| {
            set(lua, &m.0, |mat| set_float(mat, &name, v))
        });
        m.add_method("HasProperty", |lua, m, name: String| {
            get(lua, &m.0, |mat| has_property(mat, &name))
        });
        m.add_method("GetTexture", |lua, m, name: String| {
            get(lua, &m.0, |mat| {
                texture_slot(&mut mat.clone(), &name).and_then(|t| t.clone())
            })
        });
        m.add_method(
            "SetTexture",
            |lua, m, (name, path): (String, Option<String>)| {
                set(lua, &m.0, |mat| {
                    if let Some(slot) = texture_slot(mat, &name) {
                        *slot = path;
                    }
                })
            },
        );
        // The Standard shader's main maps share one tiling/offset.
        m.add_method("GetTextureOffset", |lua, m, _name: String| {
            get(lua, &m.0, |mat| vec2_value(mat.offset))
        });
        m.add_method("SetTextureOffset", |lua, m, (_name, v): (String, Value)| {
            let v = vec2(v, lua)?;
            set(lua, &m.0, |mat| mat.offset = v)
        });
        m.add_method("GetTextureScale", |lua, m, _name: String| {
            get(lua, &m.0, |mat| vec2_value(mat.tiling))
        });
        m.add_method("SetTextureScale", |lua, m, (_name, v): (String, Value)| {
            let v = vec2(v, lua)?;
            set(lua, &m.0, |mat| mat.tiling = v)
        });
        m.add_method("IsKeywordEnabled", |lua, m, k: String| {
            get(lua, &m.0, |mat| keyword(mat, &k))
        });
        // Only `_EMISSION` is a switch; the other keywords follow the material's settings.
        m.add_method("EnableKeyword", |lua, m, k: String| {
            set(lua, &m.0, |mat| {
                if k == "_EMISSION" {
                    mat.emission_enabled = true;
                }
            })
        });
        m.add_method("DisableKeyword", |lua, m, k: String| {
            set(lua, &m.0, |mat| {
                if k == "_EMISSION" {
                    mat.emission_enabled = false;
                }
            })
        });
        m.add_meta_method(MetaMethod::Eq, |_, a, b: LMaterial| Ok(a.0 == b.0));
        m.add_meta_method(MetaMethod::ToString, |lua, m, ()| {
            with_world(lua, |w| name(w, &m.0))
        });
    }
}

// ---- MeshRenderer -----------------------------------------------------------------------------

/// Use `m` for element `slot`: an asset is assigned by path, an instance's values are copied.
fn assign(w: &mut World, e: Entity, slot: usize, m: &MaterialRef) -> Result<()> {
    let values = match m {
        MaterialRef::Shared(_) => None,
        MaterialRef::Instance(..) => Some(read(w, m)?),
    };
    let mut r = w
        .scene
        .world
        .get::<&mut MeshRenderer>(e)
        .map_err(|_| gone())?;
    if r.instances.len() <= slot {
        r.instances.resize(slot + 1, None);
    }
    match m {
        MaterialRef::Shared(path) => {
            r.set_material_at(slot, path.clone());
            r.instances[slot] = None;
        }
        MaterialRef::Instance(..) => r.instances[slot] = values,
    }
    Ok(())
}

/// Element `slot` as `sharedMaterial` sees it: the renderer's copy once it has one.
fn shared_ref(w: &World, e: Entity, slot: usize) -> Result<MaterialRef> {
    renderer(w, e, |r| match r.instance(slot) {
        Some(_) => MaterialRef::Instance(e, slot),
        None => MaterialRef::Shared(r.material_at(slot).map(str::to_owned)),
    })
}

fn renderer_get<R>(lua: &Lua, e: Entity, f: impl FnOnce(&MeshRenderer) -> R) -> Result<R> {
    with_world(lua, |w| renderer(w, e, f))?
}

fn renderer_set(lua: &Lua, e: Entity, f: impl FnOnce(&mut MeshRenderer)) -> Result<()> {
    with_world(lua, |w| {
        let mut r = w
            .scene
            .world
            .get::<&mut MeshRenderer>(e)
            .map_err(|_| gone())?;
        f(&mut r);
        Ok(())
    })?
}

/// Every element as a Lua list (1-based).
fn material_list(lua: &Lua, e: Entity, instances: bool) -> Result<mlua::Table> {
    let list = with_world(lua, |w| {
        let n = renderer(w, e, MeshRenderer::material_count)?;
        (0..n)
            .map(|i| {
                if instances {
                    ensure_instance(w, e, i)?;
                    Ok(LMaterial(MaterialRef::Instance(e, i)))
                } else {
                    shared_ref(w, e, i).map(LMaterial)
                }
            })
            .collect::<Result<Vec<_>>>()
    })??;
    lua.create_sequence_from(list)
}

/// Lua list of materials for `materials = {…}` assignments.
fn assign_list(lua: &Lua, e: Entity, list: mlua::Table) -> Result<()> {
    let mats: Vec<LMaterial> = list.sequence_values().collect::<Result<_>>()?;
    with_world(lua, |w| {
        let mut r = w
            .scene
            .world
            .get::<&mut MeshRenderer>(e)
            .map_err(|_| gone())?;
        r.set_material_count(mats.len());
        r.instances.truncate(mats.len());
        drop(r);
        for (i, m) in mats.iter().enumerate() {
            assign(w, e, i, &m.0)?;
        }
        Ok(())
    })?
}

impl UserData for LMeshRenderer {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("material", |lua, r| {
            with_world(lua, |w| {
                ensure_instance(w, r.0, 0).map(|_| LMaterial(MaterialRef::Instance(r.0, 0)))
            })?
        });
        f.add_field_method_set("material", |lua, r, m: LMaterial| {
            with_world(lua, |w| assign(w, r.0, 0, &m.0))?
        });
        f.add_field_method_get("sharedMaterial", |lua, r| {
            with_world(lua, |w| shared_ref(w, r.0, 0).map(LMaterial))?
        });
        f.add_field_method_set("sharedMaterial", |lua, r, m: LMaterial| {
            with_world(lua, |w| assign(w, r.0, 0, &m.0))?
        });
        f.add_field_method_get("materials", |lua, r| material_list(lua, r.0, true));
        f.add_field_method_set("materials", |lua, r, list: mlua::Table| {
            assign_list(lua, r.0, list)
        });
        f.add_field_method_get("sharedMaterials", |lua, r| material_list(lua, r.0, false));
        f.add_field_method_set("sharedMaterials", |lua, r, list: mlua::Table| {
            assign_list(lua, r.0, list)
        });
        f.add_field_method_get("enabled", |lua, r| renderer_get(lua, r.0, |c| c.enabled));
        f.add_field_method_set("enabled", |lua, r, v: bool| {
            renderer_set(lua, r.0, |c| c.enabled = v)
        });
        f.add_field_method_get("castShadows", |lua, r| {
            renderer_get(lua, r.0, |c| c.cast_shadows)
        });
        f.add_field_method_set("castShadows", |lua, r, v: bool| {
            renderer_set(lua, r.0, |c| c.cast_shadows = v)
        });
        f.add_field_method_get("receiveShadows", |lua, r| {
            renderer_get(lua, r.0, |c| c.receive_shadows)
        });
        f.add_field_method_set("receiveShadows", |lua, r, v: bool| {
            renderer_set(lua, r.0, |c| c.receive_shadows = v)
        });
        f.add_field_method_get("gameObject", |_, r| Ok(LGameObject(r.0)));
        f.add_field_method_get("transform", |_, r| Ok(LTransform(r.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        super::override_api::add_methods(m);
    }
}

/// `GetComponent("MeshRenderer")` / `GetComponent("Renderer")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name == "SkinnedMeshRenderer" {
        return super::skinning_api::component(lua, e, name);
    }
    if !matches!(name, "MeshRenderer" | "Renderer")
        || !with_world(lua, |w| w.scene.world.get::<&MeshRenderer>(e).is_ok())?
    {
        return Ok(None);
    }
    Ok(Some(Value::UserData(
        lua.create_userdata(LMeshRenderer(e))?,
    )))
}
