# World Builder — beginner guide

The World Builder makes a whole landscape from a few settings: a base landform, erosion, stamps
(mountains, rivers, lakes, flat areas…) and biomes that texture the ground and scatter trees and rocks.
Everything is a recipe saved in the scene, so you can move a stamp and press **Generate** again.

Open it from **Window ▸ Terrain ▸ World Builder**, or click **World Builder** at the top of a
terrain's Properties panel. For a ready-made world in one click, use **Object ▸ 3D Object ▸ World** and
pick a template. To see a finished example, open `Assets/Scenes/WorldBuilderDemo.scene`.

## 1. Start

- With a terrain selected, click **Create World Builder for the Selected Terrain**. With nothing selected, click
  **Create World Builder (with a new Terrain)**.
- Or click one of the **templates** under the button: it makes the terrain and a World Builder set up
  for that kind of world.
- This adds a **World Builder** object to the Scene Tree. It also writes the biome presets and their textures
  into `Assets/WorldBuilder/`.

At the top of the window are two cards, **Preview** and **Templates**, then four sections. Click a title
to fold it away.

### Preview

A top-down picture of the world that updates while you drag any setting or stamp: a quick rough
version while you edit, then a sharper one a moment after you stop. Switch between **Height**
(grayscale), **Relief** (shading, slopes, snow and water) and **Biomes** (the ground textures the
terrain will get). **Live in Editor Scene** also shows the rough result on the terrain itself; **Generate**
replaces it with the full one, and **Revert** puts the terrain back.

### Templates

Desert, Forest, Mountains, Plains, Islands, Canyon, Tundra and Volcanic. Each sets the landform,
erosion, stamps and biomes for that kind of world (your terrain's size and resolution stay). If you
have changed the setup, the World Builder asks before replacing it; **Ctrl+Z** brings it back.

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

Every change shows up in the Preview card straight away.

## 3. Stamps & Areas

Click an **Add** button to place a stamp at the centre of the Editor Scene. Each stamp is a child object of
the World Builder:

- Move, rotate and scale it with the usual tools (W / E / R).
- Drag the white dots in the Editor Scene to change the radius and falloff, or to move river and polygon
  points.

| Stamp | What it does |
|---|---|
| Mountain | Adds a cone, ridge, volcano or mesa. *Blend*: Add raises the ground; Max, Min and Replace work from the object's height. |
| Flatten | Makes a flat area at the object's height. With *Sample Ground* on, it uses the ground height instead. |
| River | Carves a channel through its points, flowing from the first point to the last. It can add a water strip. |
| Water | *Lake* fills the basin under the object up to the object's height. *Sea Level* floods everything below it. |
| Rock Area | Rock textures and rocks take over inside. |
| Biome Zone | Uses another biome inside its circle (or polygon). |

Stamps are applied in Scene Tree order, so drag them in the Scene Tree to change which one wins.

## 4. Biomes

- **Default Biome** is used everywhere outside the Biome Zones.
- **Edge Breakup** adds noise to the edges where biomes meet.
- Click a biome name to edit it. Changes save to its `.biome` file straight away. Each biome has:
  - a texture and tiling for each kind of ground: Flat, Slope, Cliff, Shore / Underwater and Peak / Snow.
    Drag a texture from Asset Files onto a slot.
  - the angles and heights that decide where each kind of ground starts.
  - a **Scatter** list: trees, rocks, bushes or grass. For each item you can pick a prefab or a built-in
    shape, and set its density, minimum spacing, scale range, maximum slope, height band and whether it
    tilts with the ground.
  - a **Colour Tone** that tints its scatter.

## 5. Generate

- **Generate** runs everything: height → erosion → stamps → water → textures → scatter. It saves the
  terrain asset and places water and scatter under *World Builder ▸ Generated*.
- **Heights Only**, **Textures Only** and **Scatter Only** redo one part and keep the rest.
- Generate remembers each step, so after a small change only the steps it affects run again (moving a
  Biome Zone, for example, skips the heightmap and erosion).
- The progress bar shows the current step and the time left, with a **Cancel** button. **Ctrl+Z** undoes a
  whole Generate.
- **Scatter Storage**: *Objects* places one object per tree. *Compact* stores each item in a single
  **Scatter** component, which keeps scene files small and handles huge counts. *Auto* switches to Compact
  above 20 000 objects.

Tip: the same settings, stamps and seed always give the same world, so you can safely experiment.
