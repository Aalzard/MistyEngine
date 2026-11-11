Misty Engine {VERSION} - early test version
===========================================

Misty Engine is a free game engine and editor: build scenes in the Editor Scene, arrange objects in
the Scene Tree, tune them in the Properties panel, keep your files in Asset Files, and try your game
in the Scene Viewer with Play mode and Lua scripts.

This is an early test version. Things will change and some things will break.


How to start
------------

1. Unzip this folder anywhere you like (for example your Documents folder). Do not put it in
   "C:\Program Files": Misty Engine updates itself and needs to be able to write to its folder.
2. Double-click MistyEngine.exe. Misty Hub opens.
3. In Misty Hub:
   - Projects: click "New project", pick a template (MeadowDemo: a walkable meadow with a
     first-person player and free trees and textures to reuse; First Person; Racing: an
     arcade racer with Lua scripts to edit; or Floating Platform to start from scratch),
     give it a name and click Create. Or click "Open..." to open an existing project folder.
   - Learn: step-by-step tutorials. Click Start and the editor shows you where to click.
   - Settings: the folder for new projects, and the Hub's look.
4. Try the example project: click "Open..." and pick Examples\SampleProject. Open the scenes in
   Assets\Scenes (double-click them in Asset Files) to see what the engine can do.

Windows may warn that the program is from an unknown publisher. Click "More info", then
"Run anyway".


System requirements
-------------------

- Windows 10 or Windows 11, 64-bit.
- A graphics card with Vulkan or DirectX 12 (most cards from 2016 or newer).
- 4 GB of memory or more; 8 GB or more for large Landscape projects.


What is in this folder
----------------------

MistyEngine.exe            The editor and Misty Hub.
misty_player.exe           The game player. File > Build copies it into your game's build.
                           Keep it next to MistyEngine.exe.
Examples\SampleProject     Example scenes for most features.
CHANGELOG.txt              What is new.
LICENSE.txt                The license (GNU GPL v3 or later, with a runtime exception).


Updates
-------

When a new version is out, a green "Update" button appears in the Hub and in the editor's
toolbar. Click it to download, check and install the new version. Your projects are not changed.
Help > Check for Updates... checks right away.


Known limitations
-----------------

- Windows and Linux (x64). The Linux version is a separate download
  (MistyEngine-v{VERSION}-linux-x64.tar.gz on the releases page); there is no macOS version yet.
- 2D tools (sprites, 2D physics) are unfinished.
- Some menu entries are not done yet and say so in the Console.
- No crash reporter yet. If something goes wrong, please tell us what you did.
- Projects made with this version may need small changes in later versions.


Your settings live in %APPDATA%\MistyEngine (recent projects, tutorial progress, update checks).


License
-------

Misty Engine is free software under the GNU General Public License version 3 or later.
Games you make with it are yours: you can share or sell them under any license, including
closed source (see the runtime exception in LICENSE.txt).

Source code and releases: https://github.com/Aalzard/MistyEngine
