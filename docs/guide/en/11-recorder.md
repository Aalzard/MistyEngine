# Recording videos and GIFs

The **Recorder** turns your game into a video, an animated GIF or a folder of pictures. It
records at a fixed frame rate, so the result is smooth even when your computer can't run the
game that fast: while recording, every frame moves the game forward by exactly one frame of
the recording (*capture time*), however long that frame took to draw.

## Install the Recorder

The Recorder is an optional package. Projects don't have it until you ask for it:

1. Open **Window ▸ Package Manager…**.
2. Pick **Packages: Built-in** and select **Recorder**.
3. Click **Install**.

**Window ▸ General ▸ Recorder** now opens the Recorder window, and Timelines offer a
**Recorder Track**. To take it out again, select it and click **Remove**: the window, the menu
item and its settings go away. Recordings you made stay where they are.

## The Recorder window

On the left is a list of recorders. Each one has a check box (record when you press **Start
Recording**) and a name. **Add Recorder** makes a new one; right-click a recorder to duplicate
or delete it. Select one to see its settings on the right:

| Setting | What it does |
|---|---|
| Source | **Scene Viewer** (the game, as players see it), **Camera** (one camera of the scene, by name) or **Editor Scene** (the editor's view, gizmos and all) |
| Output | MP4 (H.264), WebM (AV1), GIF, PNG Sequence or JPG Sequence |
| Quality | How good videos and JPG pictures look, and how big the files get |
| Record Audio | Record the game's sound too (on by default; not for GIFs) |
| Resolution | The size of the Scene Viewer, or a size you type in |
| Frame Rate | Frames per second of the recording |
| Start / Stop | **Manual**, **Frame Range**, **Time Range** or **Timeline Clip** |
| Include UI | Draw the game's canvases (buttons, text) into the recording |
| Output Folder | Where the files go, inside the project unless you pick another folder |
| File Name | The name, with wildcards such as `<Scene>` and `<Take>` |
| Take | A number that goes up by one after every recording |

Press **Start Recording**. Recorders of the Scene Viewer or a camera enter Play mode by
themselves and leave it when they are done. The top of the window counts the frames. Press
**Stop Recording** to end a Manual recording; ranges stop on their own. When the files are
written, the window says where, with a **Show in Explorer** button.

> [!TIP]
> Recording is slower than playing on a big screen size, because every frame is saved. That
> is fine: the game waits for the recording, so no frame is ever lost.

## File names

The **File Name** box takes wildcards, filled in when the recording starts:

| Wildcard | Becomes |
|---|---|
| `<Scene>` | the scene's name |
| `<Take>` | the take number, three digits (`007`) |
| `<Frame>` | the frame number, four digits (`0042`), for picture sequences |
| `<Recorder>` | the recorder's name |
| `<Project>` | the project folder's name |
| `<Resolution>` | the size, like `1920x1080` |
| `<Source>` | Scene Viewer, Camera or Editor Scene |

The default `<Scene>_<Take>` gives `Level1_001.mp4`. Picture sequences always get a frame
number, so `Level1_001_0000.png`, `Level1_001_0001.png` and so on. With **Auto-increment** on,
a take that is already on disk is skipped instead of overwritten.

## Recording a range

**Frame Range** records frames *start* to *end* of Play mode, both included: 0 to 299 is 300
frames, ten seconds at 30 frames per second. **Time Range** does the same in seconds: 2 to 4
seconds at 30 frames per second is 60 frames. The frames before the range still run, so the
game reaches the same state it would without the recorder.

## Sound

With **Record Audio** on (it is on for new recorders), the recording keeps everything the game
plays through the Audio Listener and the Audio Mixers: music, effects, voices, as you would
hear them. MP4 and WebM files get a sound track; picture sequences get a WAV file beside the
pictures, with the same name and no frame number (`Level1_001.wav`). GIFs have no sound.

Sound stays in step with the picture even when the recording runs slower or faster than real
time: while you record, the game's sound is mixed in capture time too, one frame of sound for
each frame of picture. What you hear from the speakers while recording follows the recording,
so on a slow computer it can sound choppy. The file is not: it plays smoothly, in step.

> [!NOTE]
> Turn **Record Audio** off for a silent video, or when you will add music later in a video
> editor. When the game plays no sound at all, the sound track is silent but still there.

## Recording from a Timeline

A Timeline can record its own shots:

1. Open the Timeline window and choose **Add ▸ Recorder Track**.
2. Right-click the track and choose **Add Recorder Clip**.
3. Move and stretch the clip over the part to record, and set what it records in
   **Properties** (the same settings as the Recorder window).

In Play mode, the clip records from the moment the Timeline reaches it until it leaves it. A
five-second clip at 30 frames per second makes 150 frames.

## Formats

| Format | Good for | Notes |
|---|---|---|
| MP4 (H.264) | sharing, video editors | plays almost everywhere; sound in Opus |
| WebM (AV1) | the web | small files, slower to record; sound in Opus |
| GIF | short loops in chats and pages | 256 colours, loops forever, no sound |
| PNG Sequence | editing, compositing | one lossless picture a frame; sound as a WAV |
| JPG Sequence | quick previews | one smaller picture a frame; sound as a WAV |

Everything is built into the editor: you don't need to install anything else. Sound is saved
as Opus (48 kHz stereo), a free format that web browsers, VLC and video editors play.

> [!PROGRAMMERS]
> Scripts can record too: `Recorder.Start()` starts every enabled recorder,
> `Recorder.Start("Movie Recorder")` starts one by name, `Recorder.Stop()` stops, and
> `Recorder.isRecording` says whether a recording runs. They work in the editor with the
> package installed; a built game ignores them. Frames are copied back from the graphics card
> through a ring of buffers and encoded on worker threads, so drawing doesn't wait for disks.
> Sound comes from a tap on the final mix, which costs nothing when no recording keeps sound.
> Each block it copies carries the mix's sample clock, and while recording the mix renders only
> as far as capture time has gone, so the Recorder places every sample by that clock, not by
> when it arrived; any gap becomes silence and the track is cut to the video's exact length.
