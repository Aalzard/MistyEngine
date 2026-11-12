//! The Misty Lua API as data for autocomplete, hover info and highlighting (from docs/scripting.md).
//!
//! Each line is `owner|name|signature|doc`. Owners without `#` are global tables (`Input`), owners
//! with `#` are object types reached through a variable (`#Transform`); an empty owner is a global.

use std::sync::OnceLock;

const TABLE: &str = r#"
|Input||Keyboard, mouse and axes (the Scene Viewer must have focus)
|Time||Frame timing
|Physics||Raycasts and overlap queries
|Vector3||3D vectors
|Color||RGBA colours (0..1)
|Mathf||Math helpers
|Debug||Console logging
|GameObject||Find objects by name
|SceneManager||Load scenes from Build Settings
|LoadSceneMode||Single or Additive scene loading
|Application||Application control
|Achievements||Unlock and check achievements (stored locally)
|LayerMask||Layer names and masks
|ForceMode||Force modes for AddForce / AddTorque
|RenderSettings||The open scene's lighting values
|AudioSource||PlayClipAtPoint
|AudioMixer||Find mixers by name
|Terrain||The active terrain(s)
|Water||Water surface heights (buoyancy, wading)
|Weather||Rain, snow, wet surfaces and lightning (events: OnLightning, OnThunder, OnWeatherChanged)
|Accessibility||UI text scale and colour-blind filter (for options menus)
|Instantiate|Instantiate(prefabPathOrObject [, position])|Clone a prefab or object; returns the new GameObject
|Destroy|Destroy(gameObject)|Destroy an object at the end of the frame
|print|print(...)|Same as Debug.Log
Input|GetKey|GetKey(name)|True while the key is held ("w", "space", "left shift", …)
Input|GetKeyDown|GetKeyDown(name)|True on the frame the key was pressed
Input|GetKeyUp|GetKeyUp(name)|True on the frame the key was released
Input|GetMouseButton|GetMouseButton(button)|True while a mouse button is held (0 left, 1 right, 2 middle)
Input|GetMouseButtonDown|GetMouseButtonDown(button)|True on the frame a mouse button was pressed
Input|GetMouseButtonUp|GetMouseButtonUp(button)|True on the frame a mouse button was released
Input|GetAxis|GetAxis(name)|"Horizontal", "Vertical", "Mouse X", "Mouse Y" (-1..1)
Time|deltaTime|deltaTime|Seconds since the last frame (scaled)
Time|time|time|Seconds since Play started
Time|frameCount|frameCount|Frames since Play started
Time|fixedDeltaTime|fixedDeltaTime|Physics step (Project Settings)
Time|timeScale|timeScale|Game speed multiplier (Project Settings)
Time|unscaledDeltaTime|unscaledDeltaTime|Frame time ignoring timeScale
Physics|Raycast|Raycast(origin, direction [, maxDistance [, layerMask]])|Nearest hit or nil (point, normal, distance, gameObject, …)
Physics|RaycastAll|RaycastAll(origin, direction [, maxDistance [, layerMask]])|Every hit, nearest first
Physics|SphereCast|SphereCast(origin, radius, direction [, maxDistance [, layerMask]])|Like Raycast with a sphere
Physics|OverlapSphere|OverlapSphere(center, radius [, layerMask])|GameObjects touching a sphere
Physics|DefaultRaycastLayers|DefaultRaycastLayers|Every layer but Ignore Raycast
Physics|AllLayers|AllLayers|Every layer
Physics|IgnoreRaycastLayer|IgnoreRaycastLayer|The Ignore Raycast layer mask
Physics|gravity|gravity|World gravity (Vector3, settable while playing)
Vector3|new|new(x, y, z)|A new vector
Vector3|zero|zero|(0, 0, 0)
Vector3|one|one|(1, 1, 1)
Vector3|up|up|(0, 1, 0)
Vector3|down|down|(0, -1, 0)
Vector3|left|left|(-1, 0, 0)
Vector3|right|right|(1, 0, 0)
Vector3|forward|forward|(0, 0, 1)
Vector3|back|back|(0, 0, -1)
Vector3|Distance|Distance(a, b)|Distance between two points
Vector3|Dot|Dot(a, b)|Dot product
Vector3|Cross|Cross(a, b)|Cross product
Vector3|Lerp|Lerp(a, b, t)|Linear interpolation
#Vector3|x|x|X component
#Vector3|y|y|Y component
#Vector3|z|z|Z component
#Vector3|magnitude|magnitude|Length
#Vector3|normalized|normalized|Unit-length copy
Color|new|new(r, g, b [, a])|A new colour (0..1)
Color|white|white|(1, 1, 1, 1)
Color|black|black|(0, 0, 0, 1)
Color|red|red|(1, 0, 0, 1)
Color|green|green|(0, 1, 0, 1)
Color|blue|blue|(0, 0, 1, 1)
Color|yellow|yellow|(1, 0.92, 0.016, 1)
Color|cyan|cyan|(0, 1, 1, 1)
Color|magenta|magenta|(1, 0, 1, 1)
Color|gray|gray|(0.5, 0.5, 0.5, 1)
Color|clear|clear|(0, 0, 0, 0)
Color|Lerp|Lerp(a, b, t)|Linear interpolation
Mathf|Clamp|Clamp(value, min, max)|Value limited to min..max
Mathf|Clamp01|Clamp01(value)|Value limited to 0..1
Mathf|Lerp|Lerp(a, b, t)|Linear interpolation (t clamped)
Mathf|Abs|Abs(x)|Absolute value
Mathf|Min|Min(a, b)|Smaller value
Mathf|Max|Max(a, b)|Larger value
Mathf|Sin|Sin(radians)|Sine
Mathf|Cos|Cos(radians)|Cosine
Mathf|Sqrt|Sqrt(x)|Square root
Mathf|Floor|Floor(x)|Round down
Mathf|Round|Round(x)|Round to nearest
Mathf|Sign|Sign(x)|1 or -1
Mathf|PI|PI|3.14159…
Mathf|Infinity|Infinity|Positive infinity
Mathf|Deg2Rad|Deg2Rad|Degrees to radians factor
Mathf|Rad2Deg|Rad2Deg|Radians to degrees factor
Debug|Log|Log(...)|Write a message to the Console
Debug|LogWarning|LogWarning(...)|Write a warning to the Console
Debug|LogError|LogError(...)|Write an error to the Console (Error Pause stops on it)
GameObject|Find|Find(name)|The first active object with that name, or nil
SceneManager|LoadScene|LoadScene(nameOrIndex, mode)|Load a Build Settings scene after this frame (LoadSceneMode.Additive keeps the open ones)
SceneManager|UnloadSceneAsync|UnloadSceneAsync(nameOrIndex)|Unload an additively loaded scene after this frame
SceneManager|GetActiveScene|GetActiveScene()|The active scene (name, path, isLoaded)
SceneManager|SetActiveScene|SetActiveScene(sceneOrName)|Make a loaded scene active (its lighting is used)
SceneManager|GetSceneByName|GetSceneByName(name)|A loaded scene, or nil
SceneManager|GetSceneCount|GetSceneCount()|Number of loaded scenes
LoadSceneMode|Single|Single|Replace the open scenes
LoadSceneMode|Additive|Additive|Add to the open scenes
Application|Quit|Quit()|Close the built game
Application|HasDefine|HasDefine(name)|True when the Build Profile's scripting defines include the name ("DEMO")
Application|isDedicatedServer|isDedicatedServer|True in a Dedicated Server build (headless, no window or audio)
Application|isEditor|isEditor|True in the editor's Play mode
Achievements|Unlock|Unlock(id)|Unlock an achievement; true the first time
Achievements|IsUnlocked|IsUnlocked(id)|True when the achievement is unlocked
Achievements|Clear|Clear(id)|Lock an achievement again (testing)
Achievements|GetUnlocked|GetUnlocked()|Ids of every unlocked achievement
LayerMask|GetMask|GetMask(name, ...)|Bit mask of the named layers
LayerMask|NameToLayer|NameToLayer(name)|Layer index, or -1
LayerMask|LayerToName|LayerToName(index)|Layer name
ForceMode|Force|Force|Continuous force (uses mass and time)
ForceMode|Impulse|Impulse|Instant force (uses mass)
ForceMode|VelocityChange|VelocityChange|Instant velocity change
ForceMode|Acceleration|Acceleration|Continuous acceleration
RenderSettings|fog|fog|Fog on/off
RenderSettings|fogColor|fogColor|Fog colour
RenderSettings|fogDensity|fogDensity|Exponential fog density
RenderSettings|fogStartDistance|fogStartDistance|Linear fog start
RenderSettings|fogEndDistance|fogEndDistance|Linear fog end
RenderSettings|ambientIntensity|ambientIntensity|Environment Lighting intensity
RenderSettings|reflectionIntensity|reflectionIntensity|Environment Reflections intensity
RenderSettings|moon|moon|Moon on/off (disc, stars and moonlight at night)
RenderSettings|moonMode|moonMode|"FollowSun", "Manual" or "DateTime"
RenderSettings|moonPhase|moonPhase|Phase 0..1: 0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter
RenderSettings|moonAzimuth|moonAzimuth|Manual: degrees from north towards east
RenderSettings|moonAltitude|moonAltitude|Manual: degrees above the horizon
RenderSettings|moonDate|moonDate|DateTime: local date "YYYY-MM-DD"
RenderSettings|moonTime|moonTime|DateTime: local time in hours
RenderSettings|moonUtcOffset|moonUtcOffset|DateTime: hours east of UTC
RenderSettings|moonLatitude|moonLatitude|Observer latitude (degrees north)
RenderSettings|moonLongitude|moonLongitude|Observer longitude (degrees east)
RenderSettings|moonSize|moonSize|Moon size, multiple of its real size
RenderSettings|moonBrightness|moonBrightness|Moon disc brightness
RenderSettings|moonEarthshine|moonEarthshine|Earthshine on the dark side
RenderSettings|moonLightIntensity|moonLightIntensity|Moonlight intensity at full moon
RenderSettings|moonLightColor|moonLightColor|Moonlight colour
RenderSettings|nightSky|nightSky|Night sky on/off
RenderSettings|nightSkyDarkness|nightSkyDarkness|Night Sky Darkness: darkens the night sky colour, 0..1, stars stay
RenderSettings|stars|stars|Star field brightness at night (0 = none)
RenderSettings|starDensity|starDensity|How many stars, 1 = default, up to 3
RenderSettings|starMagnitudeSpread|starMagnitudeSpread|0 = stars about equally bright, 1 = a few very bright among many faint
RenderSettings|starGlow|starGlow|Glow and bloom around the bright stars
RenderSettings|starColorVariation|starColorVariation|Star colour temperature spread, bluish to orange
RenderSettings|starTwinkle|starTwinkle|Twinkle strength (stronger near the horizon)
RenderSettings|starTwinkleSpeed|starTwinkleSpeed|Twinkle speed
RenderSettings|starFade|starFade|How much a bright sky washes out stars and the Milky Way
RenderSettings|skyTime|skyTime|Local sidereal time in hours: turns the stars and the Milky Way (not in DateTime mode)
RenderSettings|milkyWay|milkyWay|Milky Way visibility (0 = none)
RenderSettings|milkyWayCore|milkyWayCore|Milky Way core brightness
RenderSettings|milkyWayDust|milkyWayDust|Milky Way dust lane strength
RenderSettings|milkyWayWidth|milkyWayWidth|Milky Way width multiplier
RenderSettings|lightPollution|lightPollution|Horizon glow of towns; washes out stars
RenderSettings|aurora|aurora|Aurora visibility (0 = none)
RenderSettings|auroraHeight|auroraHeight|Aurora lower edge in km
RenderSettings|auroraThickness|auroraThickness|Aurora thickness in km
RenderSettings|auroraWaviness|auroraWaviness|Aurora fold amplitude
RenderSettings|auroraRays|auroraRays|Aurora vertical ray detail
RenderSettings|auroraAzimuth|auroraAzimuth|Aurora direction in degrees from north towards east
RenderSettings|auroraDistance|auroraDistance|Aurora distance over the ground in km (far = low on the sky)
RenderSettings|auroraSpeed|auroraSpeed|Aurora animation speed
RenderSettings|auroraPulse|auroraPulse|Aurora pulsing and flicker
RenderSettings|auroraAmbient|auroraAmbient|How much the aurora tints the scene's sky light
RenderSettings|meteorRate|meteorRate|Shooting stars per minute (0 = none)
RenderSettings|meteorBrightness|meteorBrightness|Shooting star brightness
RenderSettings|milkyWayTint|milkyWayTint|Milky Way tint
RenderSettings|lightPollutionColor|lightPollutionColor|Light pollution colour
RenderSettings|auroraColor|auroraColor|Aurora colour low in the curtains
RenderSettings|auroraTopColor|auroraTopColor|Aurora colour at the curtain tops
RenderSettings|auroraCurtains|auroraCurtains|Number of aurora curtains, 1..6
RenderSettings|moonTexture|moonTexture|Moon texture path (nil = built-in)
RenderSettings|moonIllumination|moonIllumination|Lit fraction of the moon (read-only)
RenderSettings|moonDirection|moonDirection|Vector3 towards the moon (read-only)
AudioSource|PlayClipAtPoint|PlayClipAtPoint(clipPath, position [, volume [, pitch]])|A 3D one-shot at a fixed point (pitch: playback speed, 1 = normal)
AudioMixer|Find|Find(name)|The project mixer with that file name, or nil
Terrain|activeTerrain|activeTerrain|The first enabled terrain, or nil
Terrain|activeTerrains|activeTerrains|Every terrain in the scene
Water|GetHeight|GetHeight(x, z)|Height of the water surface under a point (or a Vector3), nil when dry
Weather|SetState|SetState(name [, seconds])|Blend to a weather state ("Rain", "HeavyRain", "Thunderstorm", "Snow", "Blizzard"…); false for an unknown name
Weather|ClearState|ClearState([seconds])|Hand the weather back to the Weather volumes and their timelines
Weather|SetRain|SetRain(intensity [, seconds])|Rain from 0 (none) to 1 (downpour)
Weather|SetSnow|SetSnow(intensity [, seconds])|Snow from 0 (none) to 1 (blizzard)
Weather|SetIntensity|SetIntensity(intensity [, seconds])|How hard the current rain or snow falls (0–1)
Weather|SetWind|SetWind(degrees, speed [, seconds])|Wind direction (clockwise from +Z) and speed in m/s
Weather|SetLightning|SetLightning(perMinute [, seconds])|Lightning strikes per minute (0 = none)
Weather|Strike|Strike([position [, intensity]])|A lightning strike now (thunder follows by distance / 343 m/s)
Weather|GetState|GetState()|Name of the state the weather is closest to
Weather|GetRain|GetRain()|Rain intensity at the camera (0–1)
Weather|GetSnow|GetSnow()|Snow intensity at the camera (0–1)
Weather|GetWind|GetWind()|Wind at the camera with gusts (Vector3, m/s)
Weather|GetVisibility|GetVisibility()|How far you can see through the rain or snow (m)
Weather|GetWetness|GetWetness()|Surface wetness at the camera (0–1)
Weather|GetPuddles|GetPuddles()|Puddle fill at the camera (0–1)
Weather|GetSnowCover|GetSnowCover()|Snow cover at the camera (0–1)
Weather|SetWetness|SetWetness(value)|Set surface wetness everywhere (0–1)
Weather|SetSnowCover|SetSnowCover(value)|Set snow cover everywhere (0–1)
Weather|IsScripted|IsScripted()|Whether a script is holding the weather (SetState until ClearState)
Weather|states|states|The state names, for a dropdown
Accessibility|textScale|textScale|UI text size multiplier (0.5–3, 1 = as designed)
Accessibility|colorBlindMode|colorBlindMode|"Off", "Protanopia", "Deuteranopia" or "Tritanopia"
Accessibility|filterStrength|filterStrength|Colour-blind filter strength (0–1)
Accessibility|simulate|simulate|Show how colour-blind players see the game instead of correcting it
Accessibility|colorBlindModes|colorBlindModes|The mode names, for a dropdown
#self|gameObject|gameObject|The GameObject this script is attached to
#self|transform|transform|This object's Transform
#GameObject|name|name|Object name
#GameObject|tag|tag|Tag
#GameObject|layer|layer|Layer index
#GameObject|activeSelf|activeSelf|Own active flag
#GameObject|transform|transform|The object's Transform
#GameObject|SetActive|SetActive(active)|Enable or disable the object
#GameObject|CompareTag|CompareTag(tag)|True when the tag matches
#GameObject|GetComponent|GetComponent(type)|A component ("Rigidbody", "Animator", …) or nil
#GameObject|SendCustomEvent|SendCustomEvent(name, ...)|Run the Custom Event `name` of the object's Script Graphs (and its scripts' `name` function)
#GameObject|GetVariable|GetVariable(name)|An Object variable shared with the object's Script Graphs, or nil
#GameObject|SetVariable|SetVariable(name, value)|Set an Object variable shared with the object's Script Graphs
#Transform|position|position|World position
#Transform|localPosition|localPosition|Position relative to the parent
#Transform|eulerAngles|eulerAngles|World rotation in degrees
#Transform|localEulerAngles|localEulerAngles|Local rotation in degrees
#Transform|localScale|localScale|Local scale
#Transform|forward|forward|Blue axis in world space
#Transform|right|right|Red axis in world space
#Transform|up|up|Green axis in world space
#Transform|parent|parent|Parent Transform or nil
#Transform|Translate|Translate(v)|Move by a vector
#Transform|Rotate|Rotate(eulerDegrees)|Rotate by Euler angles
#Transform|LookAt|LookAt(target)|Turn to face a point or Transform
#Rigidbody|velocity|velocity|Linear velocity
#Rigidbody|angularVelocity|angularVelocity|Angular velocity
#Rigidbody|mass|mass|Mass in kg (settable)
#Rigidbody|isKinematic|isKinematic|Moved by scripts only
#Rigidbody|useGravity|useGravity|Affected by gravity
#Rigidbody|AddForce|AddForce(v [, mode])|Apply a force (ForceMode.Force by default)
#Rigidbody|AddTorque|AddTorque(v [, mode])|Apply a torque
#Rigidbody|MovePosition|MovePosition(v)|Move a kinematic body
#Rigidbody|MoveRotation|MoveRotation(eulerDegrees)|Rotate a kinematic body
#CharacterController|Move|Move(motion)|Move by a world displacement, sliding along walls
#CharacterController|SimpleMove|SimpleMove(speed)|Move at a speed with gravity; returns isGrounded
#CharacterController|isGrounded|isGrounded|Touching the ground after the last move
#CharacterController|velocity|velocity|Velocity of the last move
#CharacterController|height|height|Capsule height
#CharacterController|radius|radius|Capsule radius
#CharacterController|center|center|Capsule centre
#CharacterController|slopeLimit|slopeLimit|Steepest climbable slope
#CharacterController|stepOffset|stepOffset|Highest climbable step
#SplineContainer|EvaluatePosition|EvaluatePosition(t)|World position at t (0–1 by distance); optional spline index
#SplineContainer|EvaluateTangent|EvaluateTangent(t)|World tangent (velocity) at t
#SplineContainer|EvaluateUpVector|EvaluateUpVector(t)|World up vector at t
#SplineContainer|EvaluateCurvature|EvaluateCurvature(t)|Curvature (1 / radius) at t
#SplineContainer|CalculateLength|CalculateLength()|World length of the spline
#SplineContainer|GetNearestPoint|GetNearestPoint(position)|Closest point on the spline and its t
#SplineContainer|KnotCount|KnotCount()|Number of knots
#SplineContainer|GetKnotPosition|GetKnotPosition(index)|World position of a knot (from 1)
#SplineContainer|splineCount|splineCount|Number of splines
#SplineAnimate|Play|Play()|Start or resume moving along the spline
#SplineAnimate|Pause|Pause()|Stop where it is
#SplineAnimate|Restart|Restart()|Back to the start and play
#SplineAnimate|normalizedTime|normalizedTime|Progress: passes done plus share of the current one
#SplineAnimate|elapsedTime|elapsedTime|Seconds played
#SplineAnimate|isPlaying|isPlaying|Moving (read-only)
#SplineAnimate|duration|duration|Seconds per pass
#WheelCollider|motorTorque|motorTorque|Drive torque on the axle (N·m)
#WheelCollider|brakeTorque|brakeTorque|Braking torque (N·m)
#WheelCollider|steerAngle|steerAngle|Steering angle in degrees
#WheelCollider|forwardStiffness|forwardStiffness|Grip multiplier of the forward friction curve
#WheelCollider|sidewaysStiffness|sidewaysStiffness|Grip multiplier of the sideways friction curve
#WheelCollider|rpm|rpm|Wheel speed in revolutions per minute (read-only)
#WheelCollider|isGrounded|isGrounded|Touching the ground (read-only)
#WheelCollider|radius|radius|Wheel radius (read-only)
#WheelCollider|GetWorldPose|GetWorldPose()|Wheel centre and Euler rotation, for a wheel model
#WheelCollider|GetGroundHit|GetGroundHit()|WheelHit table (point, normal, force, slips) or nil
#AudioSource|Play|Play()|Play the clip from the start
#AudioSource|Stop|Stop()|Stop playing
#AudioSource|Pause|Pause()|Pause
#AudioSource|UnPause|UnPause()|Resume
#AudioSource|PlayOneShot|PlayOneShot(clipPath [, volumeScale])|Play an extra copy of a clip
#AudioSource|isPlaying|isPlaying|Playing now (read-only)
#AudioSource|volume|volume|Volume 0..1
#AudioSource|pitch|pitch|Pitch
#AudioSource|loop|loop|Loop the clip
#AudioSource|mute|mute|Muted
#AudioSource|clip|clip|Clip path or nil
#AudioSource|time|time|Seconds into the clip
#AudioSource|spatialize|spatialize|Binaural (head model) 3D sound instead of panning
#AudioSource|rolloffMode|rolloffMode|"Logarithmic", "Linear" or "Custom"
#AudioSource|SetCustomCurve|SetCustomCurve("CustomRolloff", keys)|Custom rolloff keys {{distance 0..1, volume}, ...}
#AudioSource|GetCustomCurve|GetCustomCurve("CustomRolloff")|The custom rolloff keys
#AudioSource|outputAudioMixerGroup|outputAudioMixerGroup|Mixer group it plays through
#AudioMixer|SetFloat|SetFloat(name, value)|Set an exposed parameter
#AudioMixer|GetFloat|GetFloat(name)|Read an exposed parameter (nil if none)
#AudioMixer|ClearFloat|ClearFloat(name)|Back to the snapshot's value
#AudioMixer|FindSnapshot|FindSnapshot(name)|A snapshot by name, or nil
#AudioMixer|TransitionToSnapshots|TransitionToSnapshots(snapshots, weights, seconds)|Move to a weighted blend of snapshots
#AudioMixer|FindMatchingGroups|FindMatchingGroups(path)|Groups whose path ends with it
#AudioMixer|outputAudioMixerGroup|outputAudioMixerGroup|Group of another mixer it plays into
#AudioMixerSnapshot|TransitionTo|TransitionTo(seconds)|Blend to this snapshot
#Animation|Play|Play([name])|Play the default clip or a named one
#Animation|Stop|Stop()|Stop and rewind
#Animation|IsPlaying|IsPlaying(name)|True when that clip is playing
#Animation|isPlaying|isPlaying|Any clip playing
#Animation|speed|speed|Playback speed (negative = backwards)
#Animator|SetFloat|SetFloat(name, value)|Set a float parameter
#Animator|GetFloat|GetFloat(name)|Read a float parameter
#Animator|SetInteger|SetInteger(name, value)|Set an int parameter
#Animator|GetInteger|GetInteger(name)|Read an int parameter
#Animator|SetBool|SetBool(name, value)|Set a bool parameter
#Animator|GetBool|GetBool(name)|Read a bool parameter
#Animator|SetTrigger|SetTrigger(name)|Set a trigger
#Animator|ResetTrigger|ResetTrigger(name)|Clear a trigger
#Animator|Play|Play(state [, layer [, normalizedTime]])|Jump to a state
#Animator|CrossFade|CrossFade(state, normalizedDuration [, layer [, offset]])|Blend to a state
#Animator|GetCurrentAnimatorStateInfo|GetCurrentAnimatorStateInfo(layer)|normalizedTime, length, IsName(name)…
#Animator|IsInTransition|IsInTransition(layer)|True while blending
#MeshRenderer|material|material|This object's own material copy
#MeshRenderer|sharedMaterial|sharedMaterial|The material asset
#MeshRenderer|materials|materials|All material copies
#MeshRenderer|enabled|enabled|Drawn or not
#Material|color|color|Main colour
#Material|SetColor|SetColor(name, color)|Set a colour property ("_EmissionColor")
#Material|GetColor|GetColor(name)|Read a colour property
#Material|SetFloat|SetFloat(name, value)|Set a float property ("_Metallic")
#Material|GetFloat|GetFloat(name)|Read a float property
#Material|SetTexture|SetTexture(name, path)|Set a texture property
#Material|EnableKeyword|EnableKeyword(name)|Enable a shader keyword ("_EMISSION")
#Material|DisableKeyword|DisableKeyword(name)|Disable a shader keyword
#ParticleSystem|Play|Play()|Start or resume
#ParticleSystem|Stop|Stop()|Stop emitting
#ParticleSystem|Pause|Pause()|Freeze
#ParticleSystem|Clear|Clear()|Remove live particles
#ParticleSystem|Emit|Emit(count)|Emit particles now
#ParticleSystem|isPlaying|isPlaying|Playing
#ParticleSystem|particleCount|particleCount|Live particles
#VisualEffect|Play|Play()|Start spawning
#VisualEffect|Stop|Stop()|Stop spawning (live particles finish)
#VisualEffect|Reinit|Reinit()|Clear the particles and restart
#VisualEffect|SendEvent|SendEvent(name)|"OnPlay" or "OnStop"
#VisualEffect|SetFloat|SetFloat(name, value)|Override a Float property
#VisualEffect|SetVector3|SetVector3(name, v)|Override a Vector3 property
#VisualEffect|SetColor|SetColor(name, color)|Override a Color property
#VisualEffect|GetFloat|GetFloat(name)|Read a Float property
#VisualEffect|HasProperty|HasProperty(name)|The effect exposes it
#VisualEffect|ResetOverride|ResetOverride(name)|Back to the graph's default
#VisualEffect|aliveCount|aliveCount|Live particles (read back from the GPU)
#VisualEffect|isPlaying|isPlaying|Spawning
#Text|text|text|Shown text
#Text|fontSize|fontSize|Font size
#Text|color|color|Text colour
#Button|onClick|onClick|AddListener(fn) / RemoveListener / RemoveAllListeners
#Button|interactable|interactable|Accepts clicks
#Slider|value|value|Current value
#Slider|onValueChanged|onValueChanged|AddListener(fn(value))
#Toggle|isOn|isOn|Checked
#Toggle|onValueChanged|onValueChanged|AddListener(fn(isOn))
#Terrain|SampleHeight|SampleHeight(position)|Surface height under a point
#Terrain|GetPosition|GetPosition()|World position of the corner
#Terrain|terrainData|terrainData|size, heightmapResolution, …
#RaycastHit|point|point|Hit point
#RaycastHit|normal|normal|Surface normal
#RaycastHit|distance|distance|Distance from the origin
#RaycastHit|gameObject|gameObject|Object hit
#RaycastHit|rigidbody|rigidbody|Rigidbody hit or nil
"#;

/// Component names accepted by `GetComponent("…")`, with the type their members live under.
pub const COMPONENTS: &[&str] = &[
    "Transform",
    "Rigidbody",
    "CharacterController",
    "WheelCollider",
    "SplineContainer",
    "SplineAnimate",
    "AudioSource",
    "Animation",
    "Animator",
    "MeshRenderer",
    "Renderer",
    "ParticleSystem",
    "VisualEffect",
    "Terrain",
    "Text",
    "Image",
    "RawImage",
    "Button",
    "Toggle",
    "ToggleGroup",
    "Slider",
    "Scrollbar",
    "InputField",
    "Dropdown",
    "ScrollRect",
];

/// One API member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub owner: &'static str,
    pub name: &'static str,
    pub signature: &'static str,
    pub doc: &'static str,
}

impl Entry {
    pub fn is_function(&self) -> bool {
        self.signature.contains('(')
    }
}

pub fn entries() -> &'static [Entry] {
    static ENTRIES: OnceLock<Vec<Entry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        TABLE
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| {
                let mut p = l.splitn(4, '|');
                Some(Entry {
                    owner: p.next()?,
                    name: p.next()?,
                    signature: p.next()?,
                    doc: p.next()?,
                })
            })
            .collect()
    })
}

/// Members of `owner` (`"Input"`, `"#Transform"`).
pub fn members(owner: &str) -> impl Iterator<Item = &'static Entry> + '_ {
    entries().iter().filter(move |e| e.owner == owner)
}

/// Global names (tables and functions).
pub fn globals() -> impl Iterator<Item = &'static Entry> {
    members("")
}

pub fn is_global(word: &str) -> bool {
    globals().any(|e| e.name == word && !e.is_function())
}

/// The type a component name gives (`"Renderer"` → `"#MeshRenderer"`).
pub fn component_type(name: &str) -> Option<String> {
    let name = if name == "Renderer" {
        "MeshRenderer"
    } else {
        name
    };
    COMPONENTS.contains(&name).then(|| format!("#{name}"))
}

/// Info for hovering `owner.name` or a global `name`.
pub fn lookup(owner: Option<&str>, name: &str) -> Option<&'static Entry> {
    let all = entries();
    match owner {
        Some(o) => all.iter().find(|e| e.owner == o && e.name == name),
        None => all.iter().find(|e| e.owner.is_empty() && e.name == name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_parses_and_has_the_main_globals() {
        assert!(entries().len() > 150);
        for g in ["Input", "Time", "Physics", "Vector3", "Mathf", "Debug"] {
            assert!(is_global(g), "{g}");
        }
        assert!(!is_global("print"), "functions aren't coloured as tables");
        let raycast = lookup(Some("Physics"), "Raycast").unwrap();
        assert!(raycast.is_function() && raycast.signature.starts_with("Raycast("));
        assert!(members("#Transform").any(|e| e.name == "Rotate"));
        assert_eq!(component_type("Renderer").as_deref(), Some("#MeshRenderer"));
        assert_eq!(component_type("Nope"), None);
    }
}
