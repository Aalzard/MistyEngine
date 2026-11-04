# World Builder — beginner guide

The World Builder makes a whole landscape from a few settings: a base landform, erosion, stamps
(mountains, rivers, lakes, flat areas…) and biomes that texture the ground and scatter trees and rocks.
Everything is a recipe saved in the scene, so you can move a stamp and press **Generate** again.

Open it from **Window ▸ Terrain ▸ World Builder**. To see a finished example, open
`Assets/Scenes/WorldBuilderDemo.scene`.

## 1. Start

- With a terrain selected, click **Create World Builder for the Selected Terrain**. With nothing selected, click
  **Create World Builder (with a new Terrain)**.
- This adds a **World Builder** object to the Hierarchy. It also writes the biome presets and their textures
  into `Assets/WorldBuilder/`.

The window has four sections, from top to bottom. Click a section title to fold it away.

## 2. Base Heightmap

- **Terrain Size / Resolution**: width, maximum height and length in metres; heightmap samples per side.
- **Generator**: Rolling Hills, Mountains, Plateaus / Mesas, Canyons, Islands or Dunes.
- **Height**: how much of the maximum height the landform uses.
- **Scale**: the size of the main features.
- **Noise Amount**: how much the shapes are bent and broken up.
- **Roughness**: the number of detail octaves.
- **Ridge Sharpness**: how thin the mountain crests are.
- **Seed**: picks a different variation of the same settings. **Randomize** chooses a new one.
- **Erosion**:
  - *Hydraulic*: water droplets carve gullies and fill valleys. Set *Iterations* and *Strength*.
  - *Thermal*: steep slopes crumble down to the *Talus Angle*.

While **Live Preview** is on (in the Generate section), every change redraws a quick low-resolution
preview after a moment.

## 3. Stamps & Areas

Click an **Add** button to place a stamp at the centre of the Scene view. Each stamp is a child object of
the World Builder:

- Move, rotate and scale it with the usual tools (W / E / R).
- Drag the white dots in the Scene view to change the radius and falloff, or to move river and polygon
  points.

| Stamp | What it does |
|---|---|
| Mountain | Adds a cone, ridge, volcano or mesa. *Blend*: Add raises the ground; Max, Min and Replace work from the object's height. |
| Flatten | Makes a flat area at the object's height. With *Sample Ground* on, it uses the ground height instead. |
| River | Carves a channel through its points, flowing from the first point to the last. It can add a water strip. |
| Water | *Lake* fills the basin under the object up to the object's height. *Sea Level* floods everything below it. |
| Rock Area | Rock textures and rocks take over inside. |
| Biome Zone | Uses another biome inside its circle (or polygon). |

Stamps are applied in Hierarchy order, so drag them in the Hierarchy to change which one wins.

## 4. Biomes

- **Default Biome** is used everywhere outside the Biome Zones.
- **Edge Breakup** adds noise to the edges where biomes meet.
- Click a biome name to edit it. Changes save to its `.biome` file straight away. Each biome has:
  - a texture and tiling for each kind of ground: Flat, Slope, Cliff, Shore / Underwater and Peak / Snow.
    Drag a texture from the Project window onto a slot.
  - the angles and heights that decide where each kind of ground starts.
  - a **Scatter** list: trees, rocks, bushes or grass. For each item you can pick a prefab or a built-in
    shape, and set its density, minimum spacing, scale range, maximum slope, height band and whether it
    tilts with the ground.
  - a **Colour Tone** that tints its scatter.

## 5. Generate

- **Generate** runs everything: height → erosion → stamps → water → textures → scatter. It saves the
  terrain asset and places water and scatter under *World Builder ▸ Generated*.
- **Heights Only**, **Textures Only** and **Scatter Only** redo one part and keep the rest.
- The progress bar has a **Cancel** button. **Ctrl+Z** undoes a whole Generate. **Revert Preview** throws a
  preview away.
- **Scatter Storage**: *Objects* places one object per tree. *Compact* stores each item in a single
  **Scatter** component, which keeps scene files small and handles huge counts. *Auto* switches to Compact
  above 20 000 objects.

Tip: the same settings, stamps and seed always give the same world, so you can safely experiment.
