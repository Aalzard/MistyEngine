Misty Engine {VERSION} for Linux - early test version
====================================================

Misty Engine is a free game engine and editor. If you know Unity, it will feel familiar: a
Hierarchy, a Scene view, an Inspector, a Project window, Play mode and Lua scripts.

This is an early test version. Things will change and some things will break. The Linux build
is newer than the Windows one and has had less testing.


How to start
------------

1. Extract the archive somewhere in your home folder (for example ~/Applications). Misty Engine
   updates itself and needs to be able to write to its folder, so do not put it in /opt or
   /usr unless that folder belongs to you.

       tar -xzf MistyEngine-v{VERSION}-linux-x64.tar.gz -C ~/Applications

2. Start the editor:

       cd ~/Applications/MistyEngine-v{VERSION}-linux-x64
       ./MistyEngine

   Misty Hub opens. Messages and errors are written to the terminal you started it from.
3. In Misty Hub:
   - Projects: click "New project", pick a template (Standard, PP Ready, or MeadowDemo:
     a walkable meadow with a first-person player and free trees and textures to reuse),
     give it a name and click Create. Or click "Open..." to open an existing project folder.
   - Learn: step-by-step tutorials. Click Start and the editor shows you where to click.
   - Settings: the folder for new projects, and the Hub's look.
4. Try the example project: click "Open..." and pick Examples/SampleProject. Open the scenes in
   Assets/Scenes (double-click them in the Project window) to see what the engine can do.

If your file manager lost the executable bit (for example after copying the folder from a
Windows drive), run: chmod +x MistyEngine misty_player


System requirements
-------------------

- 64-bit Linux (x86_64) with glibc 2.35 or newer: Ubuntu 22.04, Debian 12, Fedora 36,
  Linux Mint 21, Pop!_OS 22.04, Arch, or newer.
- A graphics card with a Vulkan driver (most cards from 2016 or newer):
  - AMD and Intel: the Mesa Vulkan driver (package mesa-vulkan-drivers or vulkan-radeon /
    vulkan-intel).
  - NVIDIA: the proprietary NVIDIA driver (nouveau is not enough).
  "vulkaninfo --summary" (package vulkan-tools) shows whether Vulkan works.
- A desktop session on Wayland or X11.
- 4 GB of memory or more; 8 GB or more for large Landscape projects.


Libraries it needs
------------------

Most desktops have all of them already. If Misty Engine does not start, install:

  Ubuntu / Debian / Mint:
    sudo apt install libasound2 libudev1 libvulkan1 mesa-vulkan-drivers libxkbcommon0 \
        libxkbcommon-x11-0 libwayland-client0 libwayland-cursor0 libx11-6 libxcursor1 \
        libxrandr2 libxi6 libgl1 libegl1
    (on Ubuntu 24.04 and newer the sound library is called libasound2t64)

  Fedora:
    sudo dnf install alsa-lib systemd-libs vulkan-loader mesa-vulkan-drivers libxkbcommon \
        libxkbcommon-x11 libwayland-client libwayland-cursor libX11 libXcursor libXrandr libXi

  Arch:
    sudo pacman -S alsa-lib systemd-libs vulkan-icd-loader libxkbcommon libxkbcommon-x11 \
        wayland libx11 libxcursor libxrandr libxi
    (plus vulkan-radeon, vulkan-intel or nvidia-utils for your graphics card)

Sound goes through ALSA; on PipeWire or PulseAudio desktops this works through their ALSA
plugin (pipewire-alsa or pulseaudio-alsa), which is installed by default almost everywhere.

File > Open and the other file dialogs use the desktop's file chooser (xdg-desktop-portal). If
no dialog appears, install xdg-desktop-portal-gtk (or -gnome / -kde), or zenity.


Wayland and X11
---------------

Misty Engine uses Wayland when your session is Wayland, and X11 otherwise. If the window
misbehaves on Wayland (for example it cannot be moved or resized), try X11 through XWayland:

    env -u WAYLAND_DISPLAY ./MistyEngine

If the picture is wrong or the editor does not start, you can try OpenGL instead of Vulkan:

    WGPU_BACKEND=gl ./MistyEngine


What is in this folder
----------------------

MistyEngine                The editor and Misty Hub.
misty_player               The game player. File > Build copies it into your game's build.
                           Keep it next to MistyEngine.
Examples/SampleProject     Example scenes for most features.
CHANGELOG.txt              What is new.
LICENSE.txt                The license (GNU GPL v3 or later, with a runtime exception).


Updates
-------

When a new version is out, a green "Update" button appears in the Hub and in the editor's
toolbar. Click it to download, check and install the new version into this folder (the Linux
.tar.gz from the release, checked against its SHA-256). Your projects are not changed.
Help > Check for Updates... checks right away. If the folder is not writable, the button opens
the release page instead, so you can download the new version yourself.


Known limitations
-----------------

- Linux builds of your game can be made from the Linux editor. Building for Windows from Linux
  needs the Misty Engine source code and a Windows cross toolchain; macOS builds need a Mac.
- No menu entry or desktop icon is installed; start MistyEngine from the folder or make a
  launcher yourself.
- 2D tools (sprites, 2D physics) are unfinished.
- Some menu entries are not done yet and say so in the Console.
- No crash reporter yet. If something goes wrong, please tell us what you did.
- Projects made with this version may need small changes in later versions.


Your settings live in ~/.config/MistyEngine ($XDG_CONFIG_HOME/MistyEngine if set): recent
projects, tutorial progress, update checks.


License
-------

Misty Engine is free software under the GNU General Public License version 3 or later.
Games you make with it are yours: you can share or sell them under any license, including
closed source (see the runtime exception in LICENSE.txt).

Source code and releases: https://github.com/Aalzard/MistyEngine
