//! The endless-loop guard of Lua scripts: scripts that run past their time limit are stopped
//! with an error instead of freezing the editor or the game (`while true do end` in an Update).
//!
//! Lua can only stop running code from a *hook*, and a hook that counts instructions sends
//! every single instruction through a slow path while it is installed: about twice the time
//! for plain Lua work, 15-20% of a script-heavy frame. So the game's Lua state runs without
//! one. A watchdog thread looks at the armed time limits ten times a second and installs the
//! hook only on a state that ran past its limit; the hook then raises the error from inside
//! the script, and the host takes it off again when the frame ends.
//!
//! Hooks belong to one Lua thread (coroutine), and the watchdog only knows the main one. So
//! coroutines get the counting hook when they are made (they inherit it from the thread that
//! makes them: [`WATCHED_COROUTINES`]) and pay its price while they run.
//!
//! Lua states that live for one short job (reading a script's fields for the Inspector) keep
//! the plain always-on hook: [`install_deadline`].

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use mlua::{Lua, ffi};

use super::FRAME_LIMIT;

/// Instructions between two looks at the clock while the hook is installed.
const HOOK_EVERY: u32 = 10_000;

/// How often the watchdog looks at the armed limits.
const WATCH_EVERY: Duration = Duration::from_millis(100);

/// When the scripts running now must stop, and the limit that was armed (for the message).
struct Deadline {
    until: Cell<Option<Instant>>,
    limit: Cell<Duration>,
    /// The watchdog's view of this state (none: the hook is always installed).
    watched: Option<Arc<Watched>>,
}

/// What the watchdog thread shares with one Lua state.
struct Watched {
    /// Nanoseconds since [`epoch`] at which the armed scripts must stop; 0 while not armed.
    until: AtomicU64,
    /// The state's main thread and its hook, until the state goes away. Every hook change
    /// (watchdog and host) happens under this lock.
    target: Mutex<Option<Target>>,
    /// The main thread has the hook installed; the host takes it off when it arms or disarms.
    hooked: AtomicBool,
}

struct Target {
    main: *mut ffi::lua_State,
    hook: ffi::lua_Hook,
}

// SAFETY: the pointer is only used for `lua_sethook`, which Lua allows from outside the
// running thread (it is how the stand-alone interpreter handles Ctrl-C), and only under
// `Watched::target`'s lock, which the host empties before the state is closed.
unsafe impl Send for Target {}

impl Watched {
    /// Install (`on`) or remove the counting hook of the main thread.
    fn set_hook(&self, target: &Target, on: bool) {
        // SAFETY: see `Target`.
        unsafe {
            if on {
                ffi::lua_sethook(
                    target.main,
                    Some(target.hook),
                    ffi::LUA_MASKCOUNT,
                    HOOK_EVERY as _,
                );
            } else {
                ffi::lua_sethook(target.main, None, 0, 0);
            }
        }
        self.hooked.store(on, Ordering::Relaxed);
    }

    /// The watchdog's look: past the limit, the main thread gets the hook. Again on every
    /// look while it stays past it (a coroutine wrapper may have taken the hook off).
    fn check(&self, now: u64) {
        let until = self.until.load(Ordering::Relaxed);
        if until == 0 || now <= until {
            return;
        }
        let target = self.target.lock().unwrap_or_else(|e| e.into_inner());
        // Under the lock: the host may have disarmed (the frame ended) in between.
        let until = self.until.load(Ordering::Relaxed);
        if until != 0
            && now > until
            && let Some(t) = target.as_ref()
        {
            self.set_hook(t, true);
        }
    }

    /// Arm (nanoseconds since [`epoch`]) or disarm (0); a hook left on the main thread goes.
    fn arm(&self, until: u64) {
        let target = self.target.lock().unwrap_or_else(|e| e.into_inner());
        self.until.store(until, Ordering::Relaxed);
        if self.hooked.load(Ordering::Relaxed)
            && let Some(t) = target.as_ref()
        {
            self.set_hook(t, false);
        }
    }
}

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Nanoseconds from [`epoch`] to `t` (never 0: that means "not armed").
fn nanos(t: Instant) -> u64 {
    (t.saturating_duration_since(epoch()).as_nanos() as u64).max(1)
}

/// The states the watchdog thread looks at.
struct Watchdog {
    states: Mutex<Vec<Weak<Watched>>>,
    /// Signalled when the first state arrives (the thread sleeps while there is none).
    arrived: Condvar,
}

/// The watchdog, its thread started on first use (`None`: the thread could not start).
fn watchdog() -> Option<&'static Watchdog> {
    static WATCHDOG: OnceLock<Watchdog> = OnceLock::new();
    static STARTED: OnceLock<bool> = OnceLock::new();
    let dog = WATCHDOG.get_or_init(|| Watchdog {
        states: Mutex::new(Vec::new()),
        arrived: Condvar::new(),
    });
    let started = *STARTED.get_or_init(|| {
        epoch();
        std::thread::Builder::new()
            .name("misty-script-watchdog".into())
            .spawn(move || watch(dog))
            .is_ok()
    });
    started.then_some(dog)
}

fn watch(dog: &'static Watchdog) {
    loop {
        {
            let mut states = dog.states.lock().unwrap_or_else(|e| e.into_inner());
            states.retain(|s| s.strong_count() > 0);
            while states.is_empty() {
                states = dog.arrived.wait(states).unwrap_or_else(|e| e.into_inner());
            }
            let now = nanos(Instant::now());
            for s in states.iter().filter_map(Weak::upgrade) {
                s.check(now);
            }
        }
        std::thread::sleep(WATCH_EVERY);
    }
}

/// Keeps the watchdog away from a Lua state that is going away: hold it next to the `Lua`
/// and drop it first.
pub(super) struct WatchGuard(Option<Arc<Watched>>);

impl Drop for WatchGuard {
    fn drop(&mut self) {
        if let Some(w) = &self.0 {
            w.until.store(0, Ordering::Relaxed);
            *w.target.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
}

/// The hook: past the deadline, an error in the running script.
fn hook(lua: &Lua, debug: &mlua::Debug) -> mlua::Result<mlua::VmState> {
    if let Some(d) = lua.app_data_ref::<Deadline>()
        && let Some(t) = d.until.get()
        && Instant::now() > t
    {
        // A little more time so the error can unwind through the script's pcalls.
        let more = Instant::now() + Duration::from_millis(200);
        d.until.set(Some(more));
        if let Some(w) = &d.watched {
            w.until.store(nanos(more), Ordering::Relaxed);
        }
        let at = debug
            .source()
            .short_src
            .map(|s| format!("{s}:{}: ", debug.current_line().unwrap_or(0)))
            .unwrap_or_default();
        return Err(mlua::Error::runtime(format!(
            "{at}stopped after running for {} s in one frame (an endless loop?)",
            d.limit.get().as_secs()
        )));
    }
    Ok(mlua::VmState::Continue)
}

/// Guard `lua` with a hook that is always installed (simple, slower scripts): for states that
/// run one short job.
pub(super) fn install_deadline(lua: &Lua) {
    lua.set_app_data(Deadline {
        until: Cell::new(None),
        limit: Cell::new(FRAME_LIMIT),
        watched: None,
    });
    let _ = lua.set_global_hook(
        mlua::HookTriggers::new().every_nth_instruction(HOOK_EVERY),
        hook,
    );
}

/// Coroutines made from the main thread get the counting hook (a new coroutine takes the
/// hook of the thread that makes it; one made inside a coroutine has it from there). The
/// error messages of `coroutine.create` / `wrap` stay Lua's own.
const WATCHED_COROUTINES: &str = r#"
local hook = ...
local create, wrap, running = coroutine.create, coroutine.wrap, coroutine.running
coroutine.create = function(f)
    local _, main = running()
    if not main then return create(f) end
    hook(true)
    local co = create(f)
    hook(false)
    return co
end
coroutine.wrap = function(f)
    local _, main = running()
    if not main then return wrap(f) end
    hook(true)
    local w = wrap(f)
    hook(false)
    return w
end
"#;

/// Guard a game's Lua state without slowing its scripts down: the watchdog thread installs
/// the hook once the armed limit has passed (see the module). Call it on a fresh state, from
/// outside any script, and keep the guard for as long as the state lives. Falls back to
/// [`install_deadline`] if the state can't be watched.
pub(super) fn install_watched_deadline(lua: &Lua) -> WatchGuard {
    install_deadline(lua);
    let Some(dog) = watchdog() else {
        return WatchGuard(None);
    };
    // The hook function the Lua library installed on the main thread, to put back later.
    let target = lua.exec_raw_lua(|raw| {
        let main = raw.state();
        // SAFETY: plain reads of the state this call was made on.
        unsafe {
            let is_main = ffi::lua_pushthread(main) == 1;
            ffi::lua_pop(main, 1);
            let hook = ffi::lua_gethook(main)?;
            is_main.then_some(Target { main, hook })
        }
    });
    let Some(target) = target else {
        return WatchGuard(None);
    };
    let watched = Arc::new(Watched {
        until: AtomicU64::new(0),
        target: Mutex::new(None),
        hooked: AtomicBool::new(true),
    });
    let for_wrappers = watched.clone();
    let wrapped = lua
        .create_function(move |_, on: bool| {
            let target = for_wrappers
                .target
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(t) = target.as_ref() {
                for_wrappers.set_hook(t, on);
            }
            Ok(())
        })
        .and_then(|hook| {
            lua.load(WATCHED_COROUTINES)
                .set_name("=coroutine")
                .call::<()>(hook)
        });
    if wrapped.is_err() {
        // Coroutines would run unguarded: keep the always-on hook instead.
        return WatchGuard(None);
    }
    // From here on the main thread runs without the hook.
    watched.set_hook(&target, false);
    *watched.target.lock().unwrap_or_else(|e| e.into_inner()) = Some(target);
    if let Some(mut d) = lua.app_data_mut::<Deadline>() {
        d.watched = Some(watched.clone());
    }
    dog.states
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Arc::downgrade(&watched));
    dog.arrived.notify_one();
    WatchGuard(Some(watched))
}

/// Start (`on`) or stop the clock of [`FRAME_LIMIT`].
pub(super) fn arm_deadline(lua: &Lua, on: bool) {
    arm_deadline_for(lua, on.then_some(FRAME_LIMIT));
}

/// Start the clock with `limit`, or stop it (`None`).
pub(super) fn arm_deadline_for(lua: &Lua, limit: Option<Duration>) {
    if let Some(d) = lua.app_data_ref::<Deadline>() {
        let until = limit.map(|l| Instant::now() + l);
        d.until.set(until);
        if let Some(l) = limit {
            d.limit.set(l);
        }
        if let Some(w) = &d.watched {
            w.arm(until.map_or(0, nanos));
        }
    }
}
