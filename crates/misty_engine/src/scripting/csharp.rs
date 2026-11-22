//! C# scripts (preview): `.cs` files in `Assets/` run on the installed .NET runtime.
//!
//! How it works:
//! 1. **Compile.** When Play starts and the scene uses a `.cs` script (or in the background
//!    when the editor opens a project with C# scripts, [`prestart`]), every `.cs` file under
//!    `Assets/` is compiled with the SDK's `csc`, together with the engine's C# API
//!    (`dotnet/MistyEngine.cs`), into `Library/ScriptAssemblies/<hash>/Assembly-CSharp.dll`. The
//!    hash covers the sources, so an unchanged project reuses the last build.
//! 2. **Host.** The .NET runtime is loaded into the process once through `hostfxr` (found next
//!    to `dotnet`), and the assembly's `[UnmanagedCallersOnly]` entry points
//!    (`Misty.Internal.Host`) are fetched as function pointers.
//! 3. **Run.** Each `.cs` script on an object becomes an instance (a handle on the C# side)
//!    in the script host's instance list, next to the Lua scripts, with the same lifecycle.
//!    C# reaches the scene through the native function table (`csharp_native.rs`), which works
//!    on the [`World`] installed for the call.
//!
//! The Properties panel reads a script's public fields from its source
//! (`csharp_fields.rs`), so it needs neither .NET nor a compile.
//!
//! Built games: the build compiles the scripts into `<Product>_Data/Managed` ([`build_assembly`])
//! and copies the runtime next to the executable ([`bundle_runtime`]); the player loads both
//! instead of compiling or looking for an installed .NET.
//!
//! Not yet: most engine components beyond Transform, Rigidbody, Collider, AudioSource and
//! Animator, contact points, hot reload, and unloading old assemblies (each changed build stays
//! loaded until the editor exits).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use hecs::Entity;

use super::csharp_native::{Enter, NATIVE_API, NativeApi};
use super::{LogKind, World};
use crate::scene::ScriptValue;

/// The engine's C# API, compiled into every script assembly.
pub const API_SOURCE: &str = include_str!("../../dotnet/MistyEngine.cs");

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
    OnCollisionEnter = 8,
    OnCollisionExit = 9,
    OnTriggerEnter = 10,
    OnTriggerExit = 11,
    OnCollisionStay = 12,
    OnTriggerStay = 13,
    OnCollisionEnter2D = 14,
    OnCollisionExit2D = 15,
    OnTriggerEnter2D = 16,
    OnTriggerExit2D = 17,
    OnCollisionStay2D = 18,
    OnTriggerStay2D = 19,
}

const METHOD_COUNT: usize = 20;

/// Whether `path` is a C# script.
pub fn is_csharp_path(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cs"))
}

// ---- the .NET runtime ----------------------------------------------------------------------

/// hostfxr's `char_t`: UTF-16 on Windows, UTF-8 everywhere else. (The engine's own calls into
/// C# pass UTF-16 on every OS: a C# `char` is always 16 bits.)
#[cfg(windows)]
type HostChar = u16;
#[cfg(not(windows))]
type HostChar = u8;

type HostfxrInit = unsafe extern "C" fn(*const HostChar, *const c_void, *mut *mut c_void) -> i32;

/// `hostfxr_initialize_parameters` (hostfxr.h).
#[repr(C)]
struct InitParameters {
    size: usize,
    host_path: *const HostChar,
    dotnet_root: *const HostChar,
}

/// The folder a built game keeps its .NET runtime in, next to the executable.
pub const BUNDLED_RUNTIME_DIR: &str = "dotnet";

/// The data folder's subfolder holding the compiled scripts of a built game.
pub const MANAGED_DIR: &str = "Managed";

/// The runtime a built game ships with: `dotnet/` next to the executable, with a hostfxr.
fn bundled_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let root = exe.parent()?.join(BUNDLED_RUNTIME_DIR);
    find_hostfxr(&root).map(|_| root)
}
type HostfxrGetDelegate = unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> i32;
type LoadAssemblyAndGetFunctionPointer = unsafe extern "C" fn(
    *const HostChar,
    *const HostChar,
    *const HostChar,
    *const HostChar,
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

/// `s` as a NUL-terminated hostfxr string.
#[cfg(windows)]
fn wide(s: &str) -> Vec<HostChar> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
fn wide(s: &str) -> Vec<HostChar> {
    s.bytes().chain(std::iter::once(0)).collect()
}

fn wide_path(p: &Path) -> Vec<HostChar> {
    wide(&p.to_string_lossy())
}

/// Where `dotnet` lives: `DOTNET_ROOT`, else the default install folder, else `PATH`.
fn dotnet_root() -> Option<PathBuf> {
    // A built game carries its own runtime next to the executable (see [`bundle_runtime`]).
    if let Some(bundled) = bundled_root() {
        return Some(bundled);
    }
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
    // Name the dotnet folder, so a built game uses the runtime it ships with even where
    // another .NET is installed (or none is).
    let host_path = std::env::current_exe()
        .map(|p| wide_path(&p))
        .unwrap_or_else(|_| wide(""));
    let root_w = wide_path(&root);
    let params = InitParameters {
        size: std::mem::size_of::<InitParameters>(),
        host_path: host_path.as_ptr(),
        dotnet_root: root_w.as_ptr(),
    };
    let mut handle: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated strings that outlive the call, out pointer on the stack.
    let rc = unsafe {
        init(
            config.as_ptr(),
            (&params as *const InitParameters).cast(),
            &mut handle,
        )
    };
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
type CreateFn = unsafe extern "C" fn(*const u16, i32, u64, i32) -> i32;
type HasFn = unsafe extern "C" fn(i32, i32) -> i32;
type CallFn = unsafe extern "C" fn(i32, i32, u64);
type TickFn = unsafe extern "C" fn();
type ReleaseFn = unsafe extern "C" fn(i32);
type ShiftSlotsFn = unsafe extern "C" fn(u64, i32);
type SetTimeFn = unsafe extern "C" fn(f32, f32, f32, f32, f32, i32);
type SetFieldFn =
    unsafe extern "C" fn(i32, *const u16, i32, i32, f64, *const [f32; 3], *const u16, i32) -> i32;
type MessageFn = unsafe extern "C" fn(i32, *const u16, i32, i32, f64, *const u16, i32) -> i32;
type UiEventFn = unsafe extern "C" fn(u64, i32, i32, f64, *const u16, i32) -> i32;

/// The entry points of one script assembly.
#[derive(Clone, Copy)]
struct Entry {
    create: CreateFn,
    has: HasFn,
    call: CallFn,
    release: ReleaseFn,
    set_time: SetTimeFn,
    set_field: SetFieldFn,
    tick: TickFn,
    message: MessageFn,
    ui_event: UiEventFn,
    shift_slots: ShiftSlotsFn,
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
                    usize::MAX as *const HostChar,
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
                release: transmute::<P, ReleaseFn>(get("Release")?),
                set_time: transmute::<P, SetTimeFn>(get("SetTime")?),
                set_field: transmute::<P, SetFieldFn>(get("SetField")?),
                tick: transmute::<P, TickFn>(get("Tick")?),
                message: transmute::<P, MessageFn>(get("Message")?),
                ui_event: transmute::<P, UiEventFn>(get("UiEvent")?),
                shift_slots: transmute::<P, ShiftSlotsFn>(get("ShiftSlots")?),
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
            let l = l
                .replace(&format!("{}\\", project.display()), "")
                .replace(&format!("{}/", project.display()), "")
                .trim()
                .to_owned();
            // `Assets/Scripts/A.cs(12,5): error …` with forward slashes, like every other
            // project path the editor shows (the Console opens the script at that line).
            match l.find(".cs(") {
                Some(at) => format!("{}{}", l[..at].replace('\\', "/"), &l[at..]),
                None => l,
            }
        })
        // msbuild repeats each error in its closing summary.
        .filter(|l| seen.insert(l.clone()))
        .collect()
}

/// The compiler's error lines from the last compile (empty once it compiled); the Code Editor
/// marks them in open scripts.
static LAST_ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The error lines of the last compile (`Assets/…/A.cs(line,col): error CS…: …`); empty when
/// it compiled or nothing compiled yet.
pub fn last_errors() -> Vec<String> {
    LAST_ERRORS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Compile every `.cs` under `<project>/Assets` (with the API) and return the assembly path;
/// remembers the errors for [`last_errors`].
fn compile(project: &Path, timings: &mut CsTimings) -> Result<PathBuf, String> {
    let result = compile_sources(project, timings);
    let errors = match &result {
        Ok(_) => Vec::new(),
        Err(e) => e.lines().skip(1).map(str::to_owned).collect(),
    };
    *LAST_ERRORS.lock().unwrap_or_else(|e| e.into_inner()) = errors;
    result
}

/// [`compile`] without remembering the errors.
fn compile_sources(project: &Path, timings: &mut CsTimings) -> Result<PathBuf, String> {
    // One compile at a time (the background one started with the project and Play's share the
    // output folder).
    static COMPILING: Mutex<()> = Mutex::new(());
    let _one = COMPILING.lock().unwrap_or_else(|e| e.into_inner());
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

/// Starter script for Assets ▸ Create ▸ C# Script.
pub fn template(class_name: &str) -> String {
    format!(
        r#"using Misty;

// {class_name}: attach to a GameObject. Public fields show in the Properties panel.
public class {class_name} : MonoBehaviour
{{
    public float speed = 1.0f;

    // Called once before the first frame.
    void Start()
    {{
    }}

    // Called every frame.
    void Update()
    {{
    }}
}}
"#
    )
}

/// `source` with `class old` (and the constructors `old(`) renamed to `new`, for a script file
/// renamed from `old.cs` to `new.cs`; None when it has no class `old` or `new` isn't a valid
/// class name.
pub fn rename_class(source: &str, old: &str, new: &str) -> Option<String> {
    let valid = new
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && new.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !valid || old == new {
        return None;
    }
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    // Whole-word occurrences of `old` that follow `class ` or come before `(` (constructors).
    let mut out = String::with_capacity(source.len());
    let mut found_class = false;
    let mut rest = source;
    while let Some(at) = rest.find(old) {
        let before = &rest[..at];
        let after = &rest[at + old.len()..];
        let whole = !before.chars().next_back().is_some_and(ident)
            && !after.chars().next().is_some_and(ident);
        let is_class = before.trim_end().ends_with("class")
            && before
                .trim_end()
                .strip_suffix("class")
                .is_some_and(|b| !b.chars().next_back().is_some_and(ident));
        let is_ctor = after.trim_start().starts_with('(') && found_class;
        out.push_str(before);
        if whole && (is_class || is_ctor) {
            found_class |= is_class;
            out.push_str(new);
        } else {
            out.push_str(old);
        }
        rest = after;
    }
    out.push_str(rest);
    found_class.then_some(out)
}

/// For a game build: compile the project's C# scripts and copy the assembly (with its runtime
/// config) into `<data_dir>/Managed`, where the player looks first. Ok(None) for a project
/// without C# scripts; Err with the compiler's errors.
pub fn build_assembly(project: &Path, data_dir: &Path) -> Result<Option<PathBuf>, String> {
    if !project_has_scripts(project) {
        return Ok(None);
    }
    let dll = compile(project, &mut CsTimings::default())?;
    let out = data_dir.join(MANAGED_DIR);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for name in ["Assembly-CSharp.dll", "Assembly-CSharp.runtimeconfig.json"] {
        let from = dll.with_file_name(name);
        let to = out.join(name);
        std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(Some(out.join("Assembly-CSharp.dll")))
}

/// For a game build: copy this computer's .NET runtime (the host resolver and the newest
/// shared framework, about 70 MB) into `dest` (`<build>/dotnet`), so the game runs where .NET
/// isn't installed. Only the files the game needs: no SDK, no compiler. Returns the bytes
/// copied and the runtime version.
pub fn bundle_runtime(dest: &Path) -> Result<(u64, String), String> {
    let root = dotnet_root().ok_or("the .NET runtime was not found (install the .NET SDK)")?;
    let fxr = find_hostfxr(&root)
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .ok_or_else(|| format!("hostfxr was not found under {}", root.display()))?;
    let (version, framework) = newest(
        &root.join("shared").join("Microsoft.NETCore.App"),
        |p, _| {
            p.join("System.Private.CoreLib.dll")
                .is_file()
                .then(|| p.to_path_buf())
        },
    )
    .ok_or("no .NET runtime (Microsoft.NETCore.App) is installed")?;
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    let fxr_name = fxr.file_name().unwrap_or_default();
    let mut bytes = copy_tree(&fxr, &dest.join("host").join("fxr").join(fxr_name))?;
    bytes += copy_tree(
        &framework,
        &dest
            .join("shared")
            .join("Microsoft.NETCore.App")
            .join(&version),
    )?;
    // The runtime's licence travels with it (MIT; the notices are required).
    for notice in ["LICENSE.txt", "ThirdPartyNotices.txt"] {
        let from = root.join(notice);
        if from.is_file() {
            let to = dest.join(notice);
            bytes += std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok((bytes, version))
}

fn copy_tree(from: &Path, to: &Path) -> Result<u64, String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut bytes = 0;
    for entry in std::fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .flatten()
    {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            bytes += copy_tree(&path, &target)?;
        } else {
            bytes +=
                std::fs::copy(&path, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(bytes)
}

/// Whether the project has C# scripts under `Assets/`.
pub fn project_has_scripts(project: &Path) -> bool {
    let mut sources = Vec::new();
    collect_sources(&project.join("Assets"), &mut sources);
    !sources.is_empty()
}

/// Compile the project's C# scripts (reusing an unchanged build): Ok when there are none or
/// they compile, else the compiler's errors. The editor checks this before entering Play.
pub fn check_project(project: &Path) -> Result<(), String> {
    if !project_has_scripts(project) {
        return Ok(());
    }
    compile(project, &mut CsTimings::default()).map(|_| ())
}

/// What the last [`check_in_background`] found: `Some(Err(errors))` or `Some(Ok(()))` once,
/// until [`take_background_check`] takes it.
static BACKGROUND_CHECK: Mutex<Option<Result<(), String>>> = Mutex::new(None);

/// Background compiles still running (the editor keeps drawing frames until they report).
static CHECKS_RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Compile the project's C# scripts on a background thread (after a script was saved); the
/// result waits in [`take_background_check`].
pub fn check_in_background(project: &Path) {
    use std::sync::atomic::Ordering;
    let project = project.to_path_buf();
    CHECKS_RUNNING.fetch_add(1, Ordering::SeqCst);
    let spawned = std::thread::Builder::new()
        .name("csharp-check".into())
        .spawn(move || {
            if dotnet_root().is_some() {
                let result = check_project(&project);
                *BACKGROUND_CHECK.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            }
            CHECKS_RUNNING.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        CHECKS_RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Whether a background compile is still running.
pub fn background_check_running() -> bool {
    CHECKS_RUNNING.load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// The result of the last background compile, once.
pub fn take_background_check() -> Option<Result<(), String>> {
    BACKGROUND_CHECK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

/// On a background thread: compile the project's C# scripts and start the .NET runtime, so the
/// first Play doesn't wait for either. Does nothing for a project without C# scripts; errors
/// are left for Play to report.
pub fn prestart(project: &Path) {
    let project = project.to_path_buf();
    let _ = std::thread::Builder::new()
        .name("csharp-prestart".into())
        .spawn(move || {
            if !project_has_scripts(&project) || dotnet_root().is_none() {
                return;
            }
            if let Ok(dll) = compile(&project, &mut CsTimings::default()) {
                let _ = runtime(&dll.with_extension("runtimeconfig.json"));
            }
        });
}

// ---- instances -------------------------------------------------------------------------------

/// A C# script instance: its handle on the .NET side and the callbacks its class defines. The
/// script host keeps it in the same instance list as the Lua scripts, so C# scripts follow the
/// same lifecycle (Awake, OnEnable / OnDisable on SetActive and the checkbox, Start before the
/// first update, OnDestroy on Destroy and Stop, objects made by Instantiate).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CsRef {
    handle: i32,
    /// Bit per [`Method`].
    defined: u32,
}

impl CsRef {
    pub fn defines(&self, m: Method) -> bool {
        self.defined & (1 << m as i32) != 0
    }
}

/// The callback a lifecycle name stands for (None for the ones C# can't receive yet).
pub(crate) fn method_of(name: &str) -> Option<Method> {
    Some(match name {
        "Awake" => Method::Awake,
        "OnEnable" => Method::OnEnable,
        "Start" => Method::Start,
        "Update" => Method::Update,
        "FixedUpdate" => Method::FixedUpdate,
        "LateUpdate" => Method::LateUpdate,
        "OnDisable" => Method::OnDisable,
        "OnDestroy" => Method::OnDestroy,
        "OnCollisionEnter" => Method::OnCollisionEnter,
        "OnCollisionExit" => Method::OnCollisionExit,
        "OnTriggerEnter" => Method::OnTriggerEnter,
        "OnTriggerExit" => Method::OnTriggerExit,
        "OnCollisionStay" => Method::OnCollisionStay,
        "OnTriggerStay" => Method::OnTriggerStay,
        "OnCollisionEnter2D" => Method::OnCollisionEnter2D,
        "OnCollisionExit2D" => Method::OnCollisionExit2D,
        "OnTriggerEnter2D" => Method::OnTriggerEnter2D,
        "OnTriggerExit2D" => Method::OnTriggerExit2D,
        "OnCollisionStay2D" => Method::OnCollisionStay2D,
        "OnTriggerStay2D" => Method::OnTriggerStay2D,
        _ => return None,
    })
}

/// The loaded script assembly of one Play session.
#[derive(Default)]
pub(crate) struct CsHost {
    entry: Option<Entry>,
    /// Compiling or loading failed (reported once).
    failed: bool,
    pub timings: CsTimings,
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
        // A built game has its scripts compiled already (`<Product>_Data/Managed`).
        let prebuilt = project.join(MANAGED_DIR).join("Assembly-CSharp.dll");
        let assembly = if prebuilt.is_file() {
            timings.cached = true;
            Ok(prebuilt)
        } else {
            compile(project, &mut timings)
        };
        let result = assembly.and_then(|dll| {
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

    /// A C# instance of `script` (slot `slot` of `e`) with its Inspector values applied;
    /// errors go to the Console.
    pub fn create(
        &mut self,
        w: &mut World,
        project: &Path,
        e: Entity,
        slot: usize,
        script: &crate::scene::ScriptRef,
    ) -> Option<CsRef> {
        if !self.ensure_loaded(project, &mut w.logs) {
            return None;
        }
        let entry = self.entry?;
        let name = script.name();
        let name_w: Vec<u16> = name.encode_utf16().collect();
        let _enter = Enter::new(w);
        // SAFETY: entry points of the loaded assembly; the buffers outlive each call.
        unsafe {
            let handle = (entry.create)(
                name_w.as_ptr(),
                name_w.len() as i32,
                e.to_bits().get(),
                slot as i32,
            );
            if handle <= 0 {
                return None;
            }
            for (field, value) in &script.fields {
                if field == "enabled" {
                    continue;
                }
                let field_w: Vec<u16> = field.encode_utf16().collect();
                let (kind, num, v, text) = match value {
                    ScriptValue::Bool(b) => (0, f64::from(u8::from(*b)), [0.0; 3], Vec::new()),
                    ScriptValue::Number(n) => (1, *n, [0.0; 3], Vec::new()),
                    ScriptValue::Vector3(a) => (2, 0.0, *a, Vec::new()),
                    ScriptValue::Text(s) => (3, 0.0, [0.0; 3], s.encode_utf16().collect()),
                    // The entity's bits in x and y (none: 0), the asked-for kind as text.
                    ScriptValue::Object(o) => {
                        // (C# checks that it is alive.)
                        let bits = o.entity().map_or(0, |t| t.to_bits().get());
                        let v = [
                            f32::from_bits(bits as u32),
                            f32::from_bits((bits >> 32) as u32),
                            0.0,
                        ];
                        (4, 0.0, v, o.kind.encode_utf16().collect())
                    }
                };
                // A field the class no longer has keeps its saved value for later; nothing
                // to report.
                (entry.set_field)(
                    handle,
                    field_w.as_ptr(),
                    field_w.len() as i32,
                    kind,
                    num,
                    &v,
                    text.as_ptr(),
                    text.len() as i32,
                );
            }
            let mut defined = 0u32;
            for m in 0..METHOD_COUNT as i32 {
                if (entry.has)(handle, m) != 0 {
                    defined |= 1 << m;
                }
            }
            Some(CsRef { handle, defined })
        }
    }

    /// Hand this frame's clock to C# (and destroy what `Destroy(obj, t)` timed).
    pub fn set_time(&self, w: &mut World) {
        let Some(entry) = self.entry else { return };
        let t = w.time;
        let fixed = w.physics.fixed_dt;
        let _enter = Enter::new(w);
        // SAFETY: entry point of the loaded assembly.
        unsafe {
            (entry.set_time)(
                t.delta,
                t.unscaled_delta,
                t.time as f32,
                fixed,
                t.time_scale,
                t.frame as i32,
            )
        };
    }

    /// Call `m` on instance `r` (nothing when its class doesn't define it).
    pub fn call(&self, w: &mut World, r: CsRef, m: Method, other: Option<Entity>) {
        let Some(entry) = self.entry else { return };
        if !r.defines(m) {
            return;
        }
        let _enter = Enter::new(w);
        // SAFETY: entry point of the loaded assembly, valid handle.
        let other = other.map_or(0, |e| e.to_bits().get());
        unsafe { (entry.call)(r.handle, m as i32, other) };
    }

    /// Call the method `name` of instance `r` by name (SendMessage, Inspector event calls):
    /// with no argument, or one bool, number, whole number or text. Whether the class has a
    /// method of that name that takes it.
    pub fn message(&self, w: &mut World, r: CsRef, name: &str, arg: Option<&CsArg>) -> bool {
        let Some(entry) = self.entry else {
            return false;
        };
        let name_w: Vec<u16> = name.encode_utf16().collect();
        let (kind, num, text) = arg_parts(arg);
        let _enter = Enter::new(w);
        // SAFETY: entry point of the loaded assembly; the strings outlive the call.
        let found = unsafe {
            (entry.message)(
                r.handle,
                name_w.as_ptr(),
                name_w.len() as i32,
                kind,
                num,
                text.as_ptr(),
                text.len() as i32,
            )
        };
        found == 1
    }

    /// Run the C# listeners of UI event `event` on `e` (`AddListener` on `onClick`,
    /// `onValueChanged`, `onEndEdit`). Whether any ran.
    pub fn ui_event(
        &self,
        w: &mut World,
        e: Entity,
        event: UiEventKind,
        arg: Option<&CsArg>,
    ) -> bool {
        let Some(entry) = self.entry else {
            return false;
        };
        let (kind, num, text) = arg_parts(arg);
        let _enter = Enter::new(w);
        // SAFETY: entry point of the loaded assembly; the string outlives the call.
        let ran = unsafe {
            (entry.ui_event)(
                e.to_bits().get(),
                event as i32,
                kind,
                num,
                text.as_ptr(),
                text.len() as i32,
            )
        };
        ran == 1
    }

    /// Advance the coroutines (after the Update pass, like the familiar engine).
    pub fn tick(&self, w: &mut World) {
        let Some(entry) = self.entry else { return };
        let _enter = Enter::new(w);
        // SAFETY: entry point of the loaded assembly.
        unsafe { (entry.tick)() };
    }

    /// Forget instance `r` (after its OnDestroy, or when its object is gone).
    pub fn release(&self, r: CsRef) {
        if let Some(entry) = self.entry {
            // SAFETY: entry point of the loaded assembly.
            unsafe { (entry.release)(r.handle) };
        }
    }

    /// Script `removed` of `entity` left its Scripts list (`Destroy(script)`): the object's
    /// later C# scripts move down one slot.
    pub fn shift_slots(&self, entity: hecs::Entity, removed: usize) {
        if let Some(entry) = self.entry {
            // SAFETY: entry point of the loaded assembly.
            unsafe { (entry.shift_slots)(entity.to_bits().get(), removed as i32) };
        }
    }
}

/// The argument of a call by name or a UI event.
#[derive(Clone, Debug, PartialEq)]
pub enum CsArg {
    Bool(bool),
    Number(f64),
    Int(i64),
    Text(String),
}

impl CsArg {
    /// From a Lua value (None for values C# methods can't take).
    pub fn from_lua(v: &mlua::Value) -> Option<Self> {
        Some(match v {
            mlua::Value::Boolean(b) => Self::Bool(*b),
            mlua::Value::Integer(i) => Self::Int(*i),
            mlua::Value::Number(n) => Self::Number(*n),
            mlua::Value::String(s) => Self::Text(s.to_string_lossy()),
            _ => return None,
        })
    }

    pub fn from_ui(v: &crate::ui::UiValue) -> Self {
        use crate::ui::UiValue;
        match v {
            UiValue::Bool(b) => Self::Bool(*b),
            UiValue::Number(n) => Self::Number(f64::from(*n)),
            UiValue::Int(i) => Self::Int(i64::from(*i)),
            UiValue::Text(t) => Self::Text(t.clone()),
        }
    }
}

/// Kind (-1 none, 0 bool, 1 number, 2 whole number, 3 text), number and text of an argument.
fn arg_parts(arg: Option<&CsArg>) -> (i32, f64, Vec<u16>) {
    match arg {
        None => (-1, 0.0, Vec::new()),
        Some(CsArg::Bool(b)) => (0, f64::from(u8::from(*b)), Vec::new()),
        Some(CsArg::Number(n)) => (1, *n, Vec::new()),
        Some(CsArg::Int(i)) => (2, *i as f64, Vec::new()),
        Some(CsArg::Text(t)) => (3, 0.0, t.encode_utf16().collect()),
    }
}

/// UI events C# listeners can hear, in the order of `Misty.UiEvents` in MistyEngine.cs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiEventKind {
    Click = 0,
    ValueChanged = 1,
    EndEdit = 2,
}

impl UiEventKind {
    pub fn from_name(event: &str) -> Option<Self> {
        Some(match event {
            "onClick" => Self::Click,
            "onValueChanged" => Self::ValueChanged,
            "onEndEdit" => Self::EndEdit,
            _ => return None,
        })
    }
}

/// Whether a .NET runtime (installed or shipped with the game) can be found.
pub fn dotnet_available() -> bool {
    dotnet_root().is_some_and(|r| find_hostfxr(&r).is_some())
}

impl super::ScriptHost {
    /// Compile and load times of this session's C# scripts.
    pub fn csharp_timings(&self) -> CsTimings {
        self.csharp.timings
    }
}
