//! The Misty Lua API as data for autocomplete, hover info and highlighting (from docs/scripting.md).
//!
//! Each line is `owner|name|signature|doc`. Owners without `#` are global tables (`Input`), owners
//! with `#` are object types reached through a variable (`#Transform`); an empty owner is a global.

use std::sync::OnceLock;

const TABLE: &str = r#"
|Input||Keyboard, mouse and axes (the Game view must have focus)
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
|LayerMask||Layer names and masks
|ForceMode||Force modes for AddForce / AddTorque
|RenderSettings||The open scene's lighting values
|AudioSource||PlayClipAtPoint
|AudioMixer||Find mixers by name
|Terrain||The active terrain(s)
|Water||Water surface heights (buoyancy, wading)
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
AudioSource|PlayClipAtPoint|PlayClipAtPoint(clipPath, position [, volume])|A 3D one-shot at a fixed point
AudioMixer|Find|Find(name)|The project mixer with that file name, or nil
Terrain|activeTerrain|activeTerrain|The first enabled terrain, or nil
Terrain|activeTerrains|activeTerrains|Every terrain in the scene
Water|GetHeight|GetHeight(x, z)|Height of the water surface under a point (or a Vector3), nil when dry
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
#Rigidbody|mass|mass|Mass
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
#WheelCollider|motorTorque|motorTorque|Drive torque on the axle (N·m)
#WheelCollider|brakeTorque|brakeTorque|Braking torque (N·m)
#WheelCollider|steerAngle|steerAngle|Steering angle in degrees
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
    "AudioSource",
    "Animation",
    "Animator",
    "MeshRenderer",
    "Renderer",
    "ParticleSystem",
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
