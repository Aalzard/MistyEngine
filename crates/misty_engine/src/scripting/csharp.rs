//! C# scripts (proof of concept): `.cs` files in `Assets/` run on the installed .NET runtime.
//!
//! How it works:
//! 1. **Compile.** When Play starts and the scene uses a `.cs` script, every `.cs` file under
//!    `Assets/` is compiled with `dotnet build`, together with the engine's C# API
//!    (`dotnet/MistyEngine.cs`), into `Library/ScriptAssemblies/<hash>/Assembly-CSharp.dll`. The
//!    hash covers the sources, so an unchanged project reuses the last build.
//! 2. **Host.** The .NET runtime is loaded into the process once through `hostfxr` (found next
//!    to `dotnet`), and the assembly's `[UnmanagedCallersOnly]` entry points
//!    (`Misty.Internal.Host`) are fetched as function pointers.
//! 3. **Run.** Each `.cs` script on an object becomes an instance (a handle on the C# side);
//!    the script host calls Awake / OnEnable / Start / FixedUpdate / Update / LateUpdate /
//!    OnDisable / OnDestroy next to the Lua scripts. C# reaches the scene through the native
//!    function table in [`NativeApi`], which works on the [`World`] installed for the call.
//!
//! Not in the proof: Inspector fields, objects created or switched on/off during Play,
//! collision callbacks, hot reload, unloading old assemblies (each changed build stays loaded
//! until the editor exits), and builds of the player.

use std::cell::Cell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use hecs::Entity;

use super::{LogKind, World};

/// The engine's C# API, compiled into every script assembly.
const API_SOURCE: &str = include_str!("../../dotnet/MistyEngine.cs");

/// Method ids, in the order of `Host.Methods` in MistyEngine.cs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Method {
    Awake = 0,
    OnEnable = 1,
    Start = 2,
    Update = 3,
    FixedUpdate = 4,
    LateUpdate = 5,
    OnDisable = 6,
    OnDestroy = 7,
}

const METHOD_COUNT: usize = 8;

/// Whether `path` is a C# script.
pub fn is_csharp_path(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cs"))
}

// ---- the scene, for callbacks from C# ------------------------------------------------------

thread_local! {
    /// The game world while C# code runs on this thread (null otherwise).
    static WORLD: Cell<*mut World> = const { Cell::new(std::ptr::null_mut()) };
}

/// Installs `w` for C# callbacks until dropped.
struct Enter {
    previous: *mut World,
}

impl Enter {
    fn new(w: &mut World) -> Self {
        let previous = WORLD.with(|c| c.replace(w as *mut World));
        Self { previous }
    }
}

impl Drop for Enter {
    fn drop(&mut self) {
        WORLD.with(|c| c.set(self.previous));
    }
}

/// Run `f` on the world installed by [`Enter`] (None outside a C# call).
fn with_world<R>(f: impl FnOnce(&mut World) -> R) -> Option<R> {
    let p = WORLD.with(|c| c.get());
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` was set by `Enter::new` from a `&mut World` that outlives the guard, on this
    // thread, and the code holding that borrow is blocked inside the C# call that led here; the
    // C# side runs one callback at a time, so there is no other live reference.
    Some(f(unsafe { &mut *p }))
}

fn entity(id: u64) -> Option<Entity> {
    Entity::from_bits(id)
}

// ---- native functions C# calls ---------------------------------------------------------------

/// Same layout as `Misty.Internal.TransformData`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TransformData {
    position: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}

/// Same field order as `Misty.Internal.NativeApi`.
#[repr(C)]
struct NativeApi {
    log: extern "C" fn(i32, *const u16, i32),
    get_transform: extern "C" fn(u64, *mut TransformData) -> i32,
    set_transform: extern "C" fn(u64, *const TransformData) -> i32,
    euler_to_quat: extern "C" fn(*const [f32; 3], *mut [f32; 4]),
    quat_to_euler: extern "C" fn(*const [f32; 4], *mut [f32; 3]),
    get_name: extern "C" fn(u64, *mut u16, i32) -> i32,
}

static NATIVE_API: NativeApi = NativeApi {
    log: native_log,
    get_transform: native_get_transform,
    set_transform: native_set_transform,
    euler_to_quat: native_euler_to_quat,
    quat_to_euler: native_quat_to_euler,
    get_name: native_get_name,
};

extern "C" fn native_log(kind: i32, text: *const u16, len: i32) {
    if text.is_null() || len < 0 {
        return;
    }
    // SAFETY: C# passes a pinned UTF-16 string of `len` chars.
    let units = unsafe { std::slice::from_raw_parts(text, len as usize) };
    let text = String::from_utf16_lossy(units);
    let kind = match kind {
        1 => LogKind::Warning,
        2 => LogKind::Error,
        _ => LogKind::Info,
    };
    if with_world(|w| w.logs.push((kind, text.clone()))).is_none() {
        eprintln!("[C#] {text}");
    }
}

extern "C" fn native_get_transform(id: u64, out: *mut TransformData) -> i32 {
    let Some(e) = entity(id) else { return 0 };
    with_world(|w| {
        if !w.scene.contains(e) || out.is_null() {
            return 0;
        }
        let t = w.scene.transform(e);
        let data = TransformData {
            position: t.position.to_array(),
            rotation: t.rotation.to_array(),
            scale: t.scale.to_array(),
        };
        // SAFETY: `out` points at a TransformData on the C# stack.
        unsafe { out.write(data) };
        1
    })
    .unwrap_or(0)
}

extern "C" fn native_set_transform(id: u64, data: *const TransformData) -> i32 {
    let Some(e) = entity(id) else { return 0 };
    if data.is_null() {
        return 0;
    }
    // SAFETY: `data` points at a TransformData on the C# stack.
    let d = unsafe { data.read() };
    with_world(|w| {
        if !w.scene.contains(e) {
            return 0;
        }
        let mut t = w.scene.transform(e);
        t.position = Vec3::from_array(d.position);
        t.set_rotation(Quat::from_array(d.rotation).normalize());
        t.scale = Vec3::from_array(d.scale);
        w.scene.set_transform(e, t);
        super::api::sync_body(w, e);
        1
    })
    .unwrap_or(0)
}

extern "C" fn native_euler_to_quat(e: *const [f32; 3], out: *mut [f32; 4]) {
    if e.is_null() || out.is_null() {
        return;
    }
    // SAFETY: both point at structs on the C# stack.
    unsafe {
        let q = crate::camera::euler_to_quat(Vec3::from_array(*e));
        out.write(q.to_array());
    }
}

extern "C" fn native_quat_to_euler(q: *const [f32; 4], out: *mut [f32; 3]) {
    if q.is_null() || out.is_null() {
        return;
    }
    // SAFETY: both point at structs on the C# stack.
    unsafe {
        let e = crate::camera::quat_to_euler(Quat::from_array(*q).normalize());
        out.write(e.to_array());
    }
}

extern "C" fn native_get_name(id: u64, buf: *mut u16, cap: i32) -> i32 {
    let Some(e) = entity(id) else { return -1 };
    with_world(|w| {
        if !w.scene.contains(e) || buf.is_null() || cap <= 0 {
            return -1;
        }
        let name: Vec<u16> = w.scene.name(e).encode_utf16().collect();
        let n = name.len().min(cap as usize);
        // SAFETY: `buf` holds `cap` chars on the C# stack.
        unsafe { std::ptr::copy_nonoverlapping(name.as_ptr(), buf, n) };
        n as i32
    })
    .unwrap_or(-1)
}

// ---- the .NET runtime ----------------------------------------------------------------------

type HostfxrInit = unsafe extern "C" fn(*const u16, *const c_void, *mut *mut c_void) -> i32;
type HostfxrGetDelegate = unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> i32;
type LoadAssemblyAndGetFunctionPointer = unsafe extern "C" fn(
    *const u16,
    *const u16,
    *const u16,
    *const u16,
    *mut c_void,
    *mut *mut c_void,
) -> i32;

/// `hdt_load_assembly_and_get_function_pointer` in hostfxr's `coreclr_delegate_type`.
const HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER: i32 = 5;

/// The runtime, loaded once per process (.NET can't be unloaded or started twice).
struct Runtime {
    load: LoadAssemblyAndGetFunctionPointer,
}

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(p: &Path) -> Vec<u16> {
    wide(&p.to_string_lossy())
}

/// Where `dotnet` lives: `DOTNET_ROOT`, else the default install folder, else `PATH`.
fn dotnet_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("DOTNET_ROOT").map(PathBuf::from)
        && root.join(dotnet_exe_name()).is_file()
    {
        return Some(root);
    }
    #[cfg(windows)]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let root = PathBuf::from(pf).join("dotnet");
            if root.join("dotnet.exe").is_file() {
                return Some(root);
            }
        }
    }
    #[cfg(not(windows))]
    {
        for root in [
            "/usr/share/dotnet",
            "/usr/lib/dotnet",
            "/usr/local/share/dotnet",
        ] {
            let root = PathBuf::from(root);
            if root.join("dotnet").is_file() {
                return Some(root);
            }
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find(|d| d.join(dotnet_exe_name()).is_file())
    })
}

fn dotnet_exe_name() -> &'static str {
    if cfg!(windows) {
        "dotnet.exe"
    } else {
        "dotnet"
    }
}

fn hostfxr_name() -> &'static str {
    if cfg!(windows) {
        "hostfxr.dll"
    } else if cfg!(target_os = "macos") {
        "libhostfxr.dylib"
    } else {
        "libhostfxr.so"
    }
}

/// The newest `host/fxr/<version>/hostfxr` under the dotnet folder.
fn find_hostfxr(root: &Path) -> Option<PathBuf> {
    let fxr = root.join("host").join("fxr");
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for entry in std::fs::read_dir(&fxr).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let version: Vec<u64> = name
            .split(['.', '-'])
            .map_while(|p| p.parse().ok())
            .collect();
        let lib = entry.path().join(hostfxr_name());
        if lib.is_file() && best.as_ref().is_none_or(|(v, _)| version > *v) {
            best = Some((version, lib));
        }
    }
    best.map(|(_, p)| p)
}

/// Start the runtime with `runtime_config` (the first call wins; later calls reuse it).
fn runtime(runtime_config: &Path) -> Result<&'static Runtime, String> {
    RUNTIME
        .get_or_init(|| unsafe { start_runtime(runtime_config) })
        .as_ref()
        .map_err(Clone::clone)
}

unsafe fn start_runtime(runtime_config: &Path) -> Result<Runtime, String> {
    let root = dotnet_root().ok_or("the .NET runtime was not found (install the .NET SDK)")?;
    let fxr_path = find_hostfxr(&root)
        .ok_or_else(|| format!("hostfxr was not found under {}", root.display()))?;
    // SAFETY: loading the official .NET host library; it stays loaded for the process.
    let lib = unsafe { libloading::Library::new(&fxr_path) }
        .map_err(|e| format!("{}: {e}", fxr_path.display()))?;
    let lib: &'static libloading::Library = Box::leak(Box::new(lib));
    // SAFETY: symbols and signatures from hostfxr.h.
    let (init, get_delegate) = unsafe {
        let init: libloading::Symbol<HostfxrInit> = lib
            .get(b"hostfxr_initialize_for_runtime_config\0")
            .map_err(|e| e.to_string())?;
        let get: libloading::Symbol<HostfxrGetDelegate> = lib
            .get(b"hostfxr_get_runtime_delegate\0")
            .map_err(|e| e.to_string())?;
        (*init, *get)
    };
    let config = wide_path(runtime_config);
    let mut handle: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated path, no init parameters, out pointer on the stack.
    let rc = unsafe { init(config.as_ptr(), std::ptr::null(), &mut handle) };
    if rc < 0 || handle.is_null() {
        return Err(format!(
            "the .NET runtime did not start (hostfxr error {:#010x})",
            rc as u32
        ));
    }
    let mut load: *mut c_void = std::ptr::null_mut();
    // SAFETY: handle from the init call above.
    let rc = unsafe {
        get_delegate(
            handle,
            HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER,
            &mut load,
        )
    };
    if rc < 0 || load.is_null() {
        return Err(format!(
            "the .NET runtime gave no loader (hostfxr error {:#010x})",
            rc as u32
        ));
    }
    // The host context stays open: the runtime lives as long as the process.
    Ok(Runtime {
        // SAFETY: hostfxr returns a function with this signature for this delegate type.
        load: unsafe {
            std::mem::transmute::<*mut c_void, LoadAssemblyAndGetFunctionPointer>(load)
        },
    })
}

type InitFn = unsafe extern "C" fn(*const NativeApi) -> i32;
type CreateFn = unsafe extern "C" fn(*const u16, i32, u64) -> i32;
type HasFn = unsafe extern "C" fn(i32, i32) -> i32;
type CallFn = unsafe extern "C" fn(i32, i32);
type CallManyFn = unsafe extern "C" fn(*const i32, i32, i32);
type ReleaseFn = unsafe extern "C" fn(i32);
type SetTimeFn = unsafe extern "C" fn(f32, f32, f32, i64);

/// The entry points of one script assembly.
#[derive(Clone, Copy)]
struct Entry {
    create: CreateFn,
    has: HasFn,
    call: CallFn,
    call_many: CallManyFn,
    release: ReleaseFn,
    set_time: SetTimeFn,
}

impl Runtime {
    /// Load `assembly` (into its own load context) and hand it the native function table.
    fn load_assembly(&self, assembly: &Path) -> Result<Entry, String> {
        let path = wide_path(assembly);
        let type_name = wide("Misty.Internal.Host, Assembly-CSharp");
        let get = |method: &str| -> Result<*mut c_void, String> {
            let method_w = wide(method);
            let mut f: *mut c_void = std::ptr::null_mut();
            // SAFETY: NUL-terminated strings; `UNMANAGEDCALLERSONLY_METHOD` is `(char_t*)-1`.
            let rc = unsafe {
                (self.load)(
                    path.as_ptr(),
                    type_name.as_ptr(),
                    method_w.as_ptr(),
                    usize::MAX as *const u16,
                    std::ptr::null_mut(),
                    &mut f,
                )
            };
            if rc < 0 || f.is_null() {
                Err(format!(
                    "Host.{method} was not found in {} (hostfxr error {:#010x})",
                    assembly.display(),
                    rc as u32
                ))
            } else {
                Ok(f)
            }
        };
        // SAFETY: each pointer is an `[UnmanagedCallersOnly]` method with the matching signature.
        unsafe {
            use std::mem::transmute;
            type P = *mut c_void;
            let init = transmute::<P, InitFn>(get("Init")?);
            let entry = Entry {
                create: transmute::<P, CreateFn>(get("Create")?),
                has: transmute::<P, HasFn>(get("Has")?),
                call: transmute::<P, CallFn>(get("Call")?),
                call_many: transmute::<P, CallManyFn>(get("CallMany")?),
                release: transmute::<P, ReleaseFn>(get("Release")?),
                set_time: transmute::<P, SetTimeFn>(get("SetTime")?),
            };
            let count = init(&NATIVE_API);
            if count as usize != METHOD_COUNT {
                return Err(format!(
                    "the C# API and the engine disagree on callbacks ({count} vs {METHOD_COUNT})"
                ));
            }
            Ok(entry)
        }
    }
}

// ---- compiling the project's scripts --------------------------------------------------------

/// How long the last compile and load took (for the proof's numbers).
#[derive(Clone, Copy, Debug, Default)]
pub struct CsTimings {
    /// `dotnet build` (zero when an earlier build of the same sources was reused).
    pub compile: Duration,
    /// Starting the runtime (first Play only) and loading the assembly.
    pub load: Duration,
    /// Whether the build was reused.
    pub cached: bool,
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, out);
        } else if path.to_str().is_some_and(is_csharp_path) {
            out.push(path);
        }
    }
}

/// FNV-1a over the API, the sources' paths and contents, and the SDK version.
fn hash_sources(sources: &[PathBuf], root: &Path, sdk: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(API_SOURCE.as_bytes());
    feed(sdk.as_bytes());
    for s in sources {
        feed(
            s.strip_prefix(root)
                .unwrap_or(s)
                .to_string_lossy()
                .as_bytes(),
        );
        feed(&[0]);
        feed(&std::fs::read(s).unwrap_or_default());
        feed(&[0]);
    }
    h
}

fn dotnet_command(root: &Path, library: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(root.join(dotnet_exe_name()));
    // Keep NuGet and the CLI's first-run files with the project (off the system drive), and
    // quiet.
    cmd.env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_HOME", library.join("dotnet-home"))
        .env("NUGET_PACKAGES", library.join("nuget"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up from the editor.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// The SDK's major version ("11"), which picks the target framework (`net11.0`): its
/// reference pack ships with the SDK, so no download is needed.
fn sdk_major(root: &Path, library: &Path) -> Result<String, String> {
    let out = dotnet_command(root, library)
        .arg("--version")
        .output()
        .map_err(|e| format!("dotnet --version: {e}"))?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let major = v.split('.').next().unwrap_or_default().to_owned();
    if !out.status.success() || major.parse::<u32>().is_err() {
        return Err(format!("no .NET SDK found (dotnet --version said {v:?})"));
    }
    Ok(major)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The C# compiler and reference assemblies of the installed SDK, for compiling without
/// MSBuild (about 0.2 s with the compiler server warm, against 30 s or more for `dotnet build`).
struct Csc {
    /// `sdk/<version>/Roslyn/bincore/csc.dll`.
    csc: PathBuf,
    /// `packs/Microsoft.NETCore.App.Ref/<version>/ref/net<major>.0`.
    refs: PathBuf,
    /// Runtime major version the references are for ("11").
    major: String,
}

fn version_key(name: &str) -> Vec<u64> {
    name.split(['.', '-'])
        .map_while(|p| p.parse().ok())
        .collect()
}

/// The newest subfolder of `dir` (by version number) for which `inner` exists.
fn newest(dir: &Path, inner: impl Fn(&Path, &str) -> Option<PathBuf>) -> Option<(String, PathBuf)> {
    let mut best: Option<(Vec<u64>, String, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(found) = inner(&entry.path(), &name) else {
            continue;
        };
        let key = version_key(&name);
        if best.as_ref().is_none_or(|(k, _, _)| key > *k) {
            best = Some((key, name, found));
        }
    }
    best.map(|(_, n, p)| (n, p))
}

fn find_csc(root: &Path) -> Option<Csc> {
    let (_, csc) = newest(&root.join("sdk"), |p, _| {
        let c = p.join("Roslyn").join("bincore").join("csc.dll");
        c.is_file().then_some(c)
    })?;
    // Reference assemblies of a runtime that is installed (so the result can run).
    let runtimes = root.join("shared").join("Microsoft.NETCore.App");
    let (version, refs) = newest(
        &root.join("packs").join("Microsoft.NETCore.App.Ref"),
        |p, name| {
            let major = name.split('.').next()?;
            let installed = std::fs::read_dir(&runtimes).ok()?.flatten().any(|r| {
                r.file_name()
                    .to_string_lossy()
                    .split('.')
                    .next()
                    .is_some_and(|m| m == major)
            });
            let refs = p.join("ref").join(format!("net{major}.0"));
            (installed && refs.is_dir()).then_some(refs)
        },
    )?;
    let major = version.split('.').next()?.to_owned();
    Some(Csc { csc, refs, major })
}

fn quoted(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy())
}

/// Compile with `csc` straight into `out_dir`, writing the runtime config `dotnet build`
/// would. Returns the compiler's messages on failure.
fn compile_csc(
    tool: &Csc,
    root: &Path,
    library: &Path,
    sources: &[PathBuf],
    api: &Path,
    out_dir: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let mut rsp = String::from(
        "-nologo\n-target:library\n-unsafe+\n-optimize+\n-deterministic+\n-debug:portable\n\
         -nullable:disable\n-langversion:latest\n-nowarn:CS0649,CS0169,CS0414,CS8981\n",
    );
    rsp += &format!("-out:{}\n", quoted(&out_dir.join("Assembly-CSharp.dll")));
    let mut refs: Vec<PathBuf> = std::fs::read_dir(&tool.refs)
        .map_err(|e| format!("{}: {e}", tool.refs.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")))
        .collect();
    refs.sort();
    for r in &refs {
        rsp += &format!("-reference:{}\n", quoted(r));
    }
    rsp += &format!("{}\n", quoted(api));
    for s in sources {
        rsp += &format!("{}\n", quoted(s));
    }
    let rsp_path = library.join("build").join("csc.rsp");
    std::fs::write(&rsp_path, rsp).map_err(|e| format!("{}: {e}", rsp_path.display()))?;
    let out = dotnet_command(root, library)
        .arg("exec")
        .arg(&tool.csc)
        // The compiler server (VBCSCompiler) stays warm between Plays; csc falls back to
        // compiling in process when it can't reach it.
        .arg("-shared")
        .arg(format!("@{}", rsp_path.to_string_lossy()))
        .output()
        .map_err(|e| format!("csc: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let config = format!(
        r#"{{
  "runtimeOptions": {{
    "tfm": "net{m}.0",
    "rollForward": "LatestMajor",
    "framework": {{ "name": "Microsoft.NETCore.App", "version": "{m}.0.0" }}
  }}
}}
"#,
        m = tool.major
    );
    std::fs::write(out_dir.join("Assembly-CSharp.runtimeconfig.json"), config)
        .map_err(|e| format!("runtimeconfig: {e}"))
}

/// Compiler output → the error lines, with paths relative to the project.
fn error_lines(text: &str, project: &Path) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .filter(|l| l.contains("error"))
        .map(|l| {
            // Without msbuild's "[...csproj]" suffix.
            let l = l.split(" [").next().unwrap_or(l);
            l.replace(&format!("{}\\", project.display()), "")
                .replace(&format!("{}/", project.display()), "")
                .trim()
                .to_owned()
        })
        // msbuild repeats each error in its closing summary.
        .filter(|l| seen.insert(l.clone()))
        .collect()
}

/// Compile every `.cs` under `<project>/Assets` (with the API) and return the assembly path.
fn compile(project: &Path, timings: &mut CsTimings) -> Result<PathBuf, String> {
    let root = dotnet_root().ok_or("the .NET SDK was not found (install it from dot.net)")?;
    let library = project.join("Library").join("ScriptAssemblies");
    std::fs::create_dir_all(&library).map_err(|e| format!("{}: {e}", library.display()))?;
    let mut sources = Vec::new();
    collect_sources(&project.join("Assets"), &mut sources);
    let csc = find_csc(&root);
    let major = match &csc {
        Some(c) => c.major.clone(),
        None => sdk_major(&root, &library)?,
    };
    let hash = hash_sources(&sources, project, &major);
    let out_dir = library.join(format!("{hash:016x}"));
    let dll = out_dir.join("Assembly-CSharp.dll");
    if dll.is_file() && out_dir.join("Assembly-CSharp.runtimeconfig.json").is_file() {
        timings.cached = true;
        return Ok(dll);
    }
    let build = library.join("build");
    std::fs::create_dir_all(&build).map_err(|e| format!("{}: {e}", build.display()))?;
    let api = build.join("MistyEngine.cs");
    std::fs::write(&api, API_SOURCE).map_err(|e| format!("MistyEngine.cs: {e}"))?;
    if let Some(tool) = &csc {
        let started = Instant::now();
        let result = compile_csc(tool, &root, &library, &sources, &api, &out_dir);
        timings.compile = started.elapsed();
        return match result {
            Ok(()) => Ok(dll),
            Err(text) => {
                let _ = std::fs::remove_dir_all(&out_dir);
                let mut errors = error_lines(&text, project);
                if errors.is_empty() {
                    errors.push(text.trim().to_owned());
                }
                Err(format!(
                    "C# scripts did not compile:\n{}",
                    errors.join("\n")
                ))
            }
        };
    }
    // No compiler found inside the SDK: let MSBuild do it (slow, but the documented way).
    let items: String = sources
        .iter()
        .map(|s| {
            format!(
                "    <Compile Include=\"{}\" />\n",
                xml_escape(&s.to_string_lossy())
            )
        })
        .collect();
    let csproj = format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net{major}.0</TargetFramework>
    <AssemblyName>Assembly-CSharp</AssemblyName>
    <RootNamespace>Game</RootNamespace>
    <EnableDynamicLoading>true</EnableDynamicLoading>
    <RollForward>LatestMajor</RollForward>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <Nullable>disable</Nullable>
    <ImplicitUsings>disable</ImplicitUsings>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
    <GenerateAssemblyInfo>false</GenerateAssemblyInfo>
    <UseAppHost>false</UseAppHost>
    <Deterministic>true</Deterministic>
    <NoWarn>CS0649;CS0169;CS0414;CS8981</NoWarn>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="MistyEngine.cs" />
{items}  </ItemGroup>
</Project>
"#
    );
    std::fs::write(build.join("Assembly-CSharp.csproj"), csproj)
        .map_err(|e| format!("Assembly-CSharp.csproj: {e}"))?;
    let started = Instant::now();
    let out = dotnet_command(&root, &library)
        .current_dir(&build)
        .args([
            "build",
            "Assembly-CSharp.csproj",
            "-c",
            "Release",
            "-nologo",
        ])
        .args(["-v", "quiet", "-clp:NoSummary;ErrorsOnly", "-o"])
        .arg(&out_dir)
        .output()
        .map_err(|e| format!("dotnet build: {e}"))?;
    timings.compile = started.elapsed();
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        let mut errors = error_lines(&text, project);
        if errors.is_empty() {
            errors.push(format!(
                "{}{}",
                text.trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let _ = std::fs::remove_dir_all(&out_dir);
        return Err(format!(
            "C# scripts did not compile:\n{}",
            errors.join("\n")
        ));
    }
    Ok(dll)
}

// ---- instances -------------------------------------------------------------------------------

pub(crate) struct CsInstance {
    pub entity: Entity,
    /// Index into the object's `Scripts` list.
    pub slot: usize,
    pub name: String,
    handle: i32,
    /// Which callbacks the class defines (bit per [`Method`]).
    defined: u8,
    pub awoken: bool,
    pub started: bool,
    pub enabled: bool,
}

impl CsInstance {
    fn defines(&self, m: Method) -> bool {
        self.defined & (1 << m as i32) != 0
    }
}

/// The C# scripts of one Play session.
#[derive(Default)]
pub(crate) struct CsHost {
    entry: Option<Entry>,
    /// Why C# is unavailable (reported once).
    failed: bool,
    pub instances: Vec<CsInstance>,
    pub timings: CsTimings,
    /// Handles scratch for batched calls.
    batch: Vec<i32>,
}

impl CsHost {
    /// Compile and load the project's scripts (once per session). Errors go to `logs`.
    fn ensure_loaded(&mut self, project: &Path, logs: &mut Vec<(LogKind, String)>) -> bool {
        if self.entry.is_some() {
            return true;
        }
        if self.failed {
            return false;
        }
        let mut timings = CsTimings::default();
        let result = compile(project, &mut timings).and_then(|dll| {
            let started = Instant::now();
            let config = dll.with_extension("runtimeconfig.json");
            let entry = runtime(&config)?.load_assembly(&dll);
            timings.load = started.elapsed();
            entry
        });
        self.timings = timings;
        match result {
            Ok(entry) => {
                self.entry = Some(entry);
                true
            }
            Err(e) => {
                self.failed = true;
                logs.push((LogKind::Error, e));
                false
            }
        }
    }

    /// A C# instance of the script at `path` (slot `slot` of `e`); errors go to the Console.
    pub fn instantiate(
        &mut self,
        w: &mut World,
        project: &Path,
        e: Entity,
        slot: usize,
        path: &str,
    ) {
        if !self.ensure_loaded(project, &mut w.logs) {
            return;
        }
        let Some(entry) = self.entry else { return };
        let name = Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name_w: Vec<u16> = name.encode_utf16().collect();
        let _enter = Enter::new(w);
        // SAFETY: entry points of the loaded assembly; the name buffer outlives the call.
        let handle =
            unsafe { (entry.create)(name_w.as_ptr(), name_w.len() as i32, e.to_bits().get()) };
        if handle <= 0 {
            return;
        }
        let mut defined = 0u8;
        for m in 0..METHOD_COUNT as i32 {
            // SAFETY: as above.
            if unsafe { (entry.has)(handle, m) } != 0 {
                defined |= 1 << m;
            }
        }
        self.instances.push(CsInstance {
            entity: e,
            slot,
            name,
            handle,
            defined,
            awoken: false,
            started: false,
            enabled: false,
        });
    }

    /// Hand the game clock to C#.
    pub fn set_time(&self, time: &crate::runtime::Time, fixed_dt: f32) {
        if let Some(entry) = self.entry {
            // SAFETY: entry point of the loaded assembly.
            unsafe { (entry.set_time)(time.delta, time.time as f32, fixed_dt, time.frame as i64) };
        }
    }

    /// Call `m` on instance `i`.
    pub fn call(&self, w: &mut World, i: usize, m: Method) {
        let (Some(entry), Some(inst)) = (self.entry, self.instances.get(i)) else {
            return;
        };
        if !inst.defines(m) {
            return;
        }
        crate::profile_scope!(format!("{}.{m:?}()", inst.name));
        let _enter = Enter::new(w);
        // SAFETY: entry point of the loaded assembly, valid handle.
        unsafe { (entry.call)(inst.handle, m as i32) };
    }

    /// Call `m` on the instances in `which` that define it, in one transition into .NET.
    pub fn call_many(&mut self, w: &mut World, which: &[usize], m: Method) {
        let Some(entry) = self.entry else { return };
        self.batch.clear();
        self.batch.extend(
            which
                .iter()
                .filter_map(|&i| self.instances.get(i))
                .filter(|inst| inst.defines(m))
                .map(|inst| inst.handle),
        );
        if self.batch.is_empty() {
            return;
        }
        let _enter = Enter::new(w);
        // SAFETY: entry point of the loaded assembly; the handle buffer outlives the call.
        unsafe { (entry.call_many)(self.batch.as_ptr(), self.batch.len() as i32, m as i32) };
    }

    /// Forget every instance (after OnDestroy).
    pub fn release_all(&mut self) {
        if let Some(entry) = self.entry {
            for inst in &self.instances {
                // SAFETY: entry point of the loaded assembly.
                unsafe { (entry.release)(inst.handle) };
            }
        }
        self.instances.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
}

// ---- the script host's C# passes ------------------------------------------------------------

impl super::ScriptHost {
    /// Whether C# instance `i` should get callbacks (object alive and active, script enabled).
    fn csharp_runnable(&self, i: usize) -> bool {
        let Some(inst) = self.csharp.instances.get(i) else {
            return false;
        };
        let (e, slot) = (inst.entity, inst.slot);
        self.with_world(|w| {
            w.scene.contains(e)
                && w.scene.active_in_hierarchy(e)
                && w.scene
                    .world
                    .get::<&crate::scene::Scripts>(e)
                    .ok()
                    .and_then(|s| s.list.get(slot).map(|r| r.enabled))
                    .unwrap_or(false)
        })
    }

    fn csharp_call(&mut self, i: usize, m: Method) {
        let mut w = self
            .lua
            .app_data_mut::<World>()
            .expect("world is installed");
        self.csharp.call(&mut w, i, m);
    }

    /// As Play starts, after the Lua scripts' Awake: Awake then OnEnable, script by script.
    pub(super) fn csharp_awake(&mut self) {
        if self.csharp.is_empty() {
            return;
        }
        crate::profile_scope!("Scripts.Start.CSharp");
        for i in 0..self.csharp.instances.len() {
            if !self.csharp_runnable(i) {
                continue;
            }
            self.csharp.instances[i].awoken = true;
            self.csharp_call(i, Method::Awake);
            if self.csharp_runnable(i) {
                self.csharp.instances[i].enabled = true;
                self.csharp_call(i, Method::OnEnable);
            }
            self.flush_spawned();
        }
    }

    /// Each frame, after the Lua scripts' Start: the clock, then Start for new instances.
    pub(super) fn csharp_start(&mut self, time: &crate::runtime::Time) {
        if self.csharp.is_empty() {
            return;
        }
        let fixed = self.with_world(|w| w.physics.fixed_dt);
        self.csharp.set_time(time, fixed);
        for i in 0..self.csharp.instances.len() {
            let inst = &self.csharp.instances[i];
            if inst.started || !inst.enabled || !self.csharp_runnable(i) {
                continue;
            }
            self.csharp.instances[i].started = true;
            self.csharp_call(i, Method::Start);
            self.flush_spawned();
        }
    }

    /// FixedUpdate, Update or LateUpdate on every started C# instance, in one call into .NET.
    pub(super) fn csharp_pass(&mut self, m: Method) {
        if self.csharp.is_empty() {
            return;
        }
        let which: Vec<usize> = (0..self.csharp.instances.len())
            .filter(|&i| self.csharp.instances[i].started && self.csharp_runnable(i))
            .collect();
        {
            let mut w = self
                .lua
                .app_data_mut::<World>()
                .expect("world is installed");
            self.csharp.call_many(&mut w, &which, m);
        }
        self.flush_spawned();
    }

    /// The game stops: OnDisable and OnDestroy, then the instances are released.
    pub(super) fn csharp_shutdown(&mut self) {
        for i in 0..self.csharp.instances.len() {
            if self.csharp.instances[i].enabled {
                self.csharp.instances[i].enabled = false;
                self.csharp_call(i, Method::OnDisable);
            }
            if self.csharp.instances[i].awoken {
                self.csharp_call(i, Method::OnDestroy);
            }
        }
        self.csharp.release_all();
    }

    /// Compile and load times of this session's C# scripts.
    pub fn csharp_timings(&self) -> CsTimings {
        self.csharp.timings
    }
}

#[cfg(test)]
pub(crate) fn dotnet_available() -> bool {
    dotnet_root().is_some_and(|r| find_hostfxr(&r).is_some())
}
