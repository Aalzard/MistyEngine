# Prefabs 2.0 (round 18A, branch r18-prefabs)

Goal: the everyday prefab workflow of the familiar editor: Prefab Mode, nested prefabs,
variants, per-property overrides that survive everything, unpacking, model files as prefabs.
The behaviour follows the familiar editor's; the code and the file format are our own.

## What works

| Feature | How it behaves | Where |
|---|---|---|
| Per-property overrides | An instance's overrides are what differs from its asset, one component field at a time ("Transform ▸ Scale"), plus added / removed components and added / removed child objects. The root's name, position and rotation, the component order and per-object save / network ids are always the instance's own and never count. | `scene/prefab/diff.rs` |
| Stable object ids | Each prefab object carries `"Prefab Source": { "id" }`; an instance copies them, so a renamed or moved child is still the same object and an added child is recognised as added. Objects inside a nested instance combine the nested root's id with their own, so two wheels in one cart never share ids. Older files without ids match by parent, name and order (the same rule on both sides). | `scene/prefab/ids.rs` |
| Following the asset | When an asset changes, every instance is updated **in place** (same entities, so the selection and references from other objects survive), keeping what it overrode. Used by Apply, Prefab Mode saves, live reload of a changed `.prefab` file, and variants / nested prefabs. | `scene/prefab/sync.rs`, editor `prefabs::propagate` |
| Overrides ▾ | The Inspector's Prefab row on an outermost instance root: Open, Select, **Overrides (n) ▾** listing each change grouped by object with Revert / Apply, then Revert All / Apply All. | `inspector/prefab_overrides.rs` |
| Bold overridden values | Overridden property labels are drawn bold with a blue bar in the margin; right-click ▸ Apply to Prefab 'Cart' (or to a nested prefab 'Wheel' the object comes from) / Revert. Component headers with changes get the blue bar ("+" for added components) and Apply / Revert in their ⋮ menu; removed components are listed (struck through) with Revert / Apply. | `inspector/prefab_overrides.rs` |
| Per-item Apply / Revert | `Scene::apply_prefab_overrides` copies chosen overrides into the asset (objects the instance added get ids on both sides); `revert_prefab_overrides` puts chosen ones back. Apply All on one instance keeps the other instances' own overrides. | `scene/prefab/apply.rs` |
| Prefab Mode | Double-click a prefab, the › arrow on an instance row, Open in the Inspector, or Open Prefab on the asset. The prefab is edited alone in place of the scenes (their undo history, selection, expanded rows and Scene view camera come back on exit). Breadcrumb bar above the Scene view: ◀, "Scenes ▸ Cart ▸ Wheel", **In Context**, **Auto Save**, **Save** (Ctrl+S). Opening a prefab from inside Prefab Mode goes one level deeper. Leaving with unsaved changes asks Save / Discard / Cancel in the bar (no modal dialog). Play, quitting and opening a scene leave Prefab Mode first. | `prefab_mode.rs` |
| In context | Opened from an instance, the scene around it is drawn dimmed and greyed behind the prefab (the instance itself hidden, the scene moved so the instance stands where the prefab root is). | `prefab_mode::context_draws` |
| Nested prefabs | Instances inside prefabs keep their links. When the inner prefab changes, every prefab file using it is updated and saved (keeping the outer prefab's overrides on it), then their instances follow, recursively. A prefab can't be put inside itself. | `Prefab::follow`, `prefabs::propagate` |
| Prefab Variants | Project ▸ Create ▸ Prefab Variant (or Create Variant on the asset): a prefab whose root is an instance of its base. Base changes flow into variants (and variants of variants); the variant's own changes win. In Prefab Mode a variant's changes show as overrides of its base (Overrides ▾ works there). | `Prefab::variant_of`, `Prefab::follow` |
| Unpack | Unpack Prefab (nested instances stay linked) and Unpack Completely, from the Hierarchy's Prefab menu; Select Asset. | `Scene::unpack_prefab` |
| Missing prefab | Instances whose file is gone are drawn red in the Hierarchy with "Missing Prefab"; the Inspector shows the red state with Unpack. | `panels/prefab_rows.rs` |
| Hierarchy | Blue instance names, › opens Prefab Mode, "+" badge on objects an instance added; Prefab menu: Open, Select Asset, Unpack, Unpack Completely, Apply / Revert Added GameObject. In Prefab Mode the prefab's header (◀ name) replaces the scene headers. | `panels/prefab_rows.rs`, `hierarchy.rs` |
| Model files | Dropped `.glb`/`.gltf` models are linked to their file as read-only prefabs: overrides show and revert; Apply is disabled; Create ▸ Prefab Variant of a model works. | `model_import::model_prefab` |

## File format (backward compatible)

- **Scene and prefab objects** may carry `"Prefab Source": { "id": 123 }` (ids < 2^53). Written
  for prefabs the editor creates or saves and for their instances; generator output and older
  files have none and still match.
- **Instance links** may carry a record: `"Prefab": { "path": "…", "overrides": [ { "kind":
  "property", "object": 0, "component": "Transform", "property": "scale" }, … ] }`, written
  when the scene (or prefab) is saved. Kinds: `property`, `added_component`,
  `removed_component`, `added_object` (`parent`, `name`), `removed_object`. When the asset
  changed since, the instance catches up on load keeping exactly those overrides (and the
  scene is marked changed). Files without the key load as before and are left alone.
- **Variants**: a prefab file whose root object has a `"Prefab"` link is a variant of that
  prefab (the record lists the variant's changes).
- The format version stays 1: older editors skip the new component with a warning.
- SampleProject loads unchanged; every `*_matches_its_generator` test passes (generators write
  no ids).

## Tests

- Engine (`scene/prefab/tests.rs`, `ids.rs`, `diff.rs`): per-property diff, revert one, apply
  one with other instances keeping theirs, Apply All, added objects and components, removed
  objects staying removed, references inside the prefab and to scene objects surviving, a
  variant chain, nested propagation, record catch-up, file round trips, unpack.
- Editor (`prefabs/tests.rs`): per-item apply / revert with undo, variants following their base,
  nested prefab files updated, save → edit prefab on disk → open catches up, missing prefabs,
  Prefab Mode edit / save / discard / nested levels / Auto Save.
- kittest (`inspector/mod.rs`): Overrides dropdown (list, Revert one, Apply one, Apply All,
  Revert All), bold override styling, removed components, Missing Prefab, Prefab Mode bar
  (Save, ◀).

## Screenshots

`docs/screenshots/r18/prefabs_*.png`.

## Not done yet

- Scene files not open in the editor keep their old copies of an instance until opened (they
  catch up then); a build made without opening them packs the old copies.
- Overridden labels are matched by name: a few editors whose label differs from the field
  name only show the header bar (the Overrides list always has every change).
- No side-by-side comparison view in the Overrides dropdown; no per-element array overrides
  (a list field is one property).
- Reparenting objects inside an instance isn't tracked as an override (the familiar editor
  forbids it); following the asset puts them back under their prefab parent.
- Model prefabs refresh when the model file changes, not when its extracted materials do.
