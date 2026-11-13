//! Test helpers that tell a slow frame of ours from a loaded machine: the CPU time a thread
//! actually ran (it does not grow while the thread waits for a core) and how busy every core
//! was. Timing tests compare these with wall-clock times (`render::hitch_tests`, the World
//! Partition flight).

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThread() -> isize;
    fn QueryThreadCycleTime(thread: isize, cycles: *mut u64) -> i32;
    fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
}

/// CPU cycles this thread has run (kernel and user), counted only while it is on a core.
#[cfg(windows)]
pub fn thread_work() -> Option<u64> {
    let mut cycles = 0u64;
    // SAFETY: the current thread's pseudo handle needs no closing; `cycles` is writable.
    let ok = unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
    (ok != 0).then_some(cycles)
}

/// (idle, total) CPU time of every core since boot, in 100 ns units.
#[cfg(windows)]
pub fn system_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = Default::default();
    // SAFETY: three writable FILETIMEs.
    let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    let t = |f: &FileTime| (u64::from(f.high) << 32) | u64::from(f.low);
    // Kernel time includes the idle time.
    (ok != 0).then(|| (t(&idle), t(&kernel) + t(&user)))
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
}

/// Nanoseconds of CPU time this thread has used.
#[cfg(target_os = "linux")]
pub fn thread_work() -> Option<u64> {
    const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
    let mut ts = Timespec { sec: 0, nsec: 0 };
    // SAFETY: `ts` is a writable timespec (64-bit fields on the 64-bit targets built).
    let ok = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0;
    ok.then(|| ts.sec as u64 * 1_000_000_000 + ts.nsec as u64)
}

/// (idle, total) CPU time of every core since boot, in clock ticks (`/proc/stat`).
#[cfg(target_os = "linux")]
pub fn system_times() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let fields: Vec<u64> = text
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .filter_map(|f| f.parse().ok())
        .collect();
    // user nice system idle iowait ...
    let idle = fields.get(3)? + fields.get(4).copied().unwrap_or(0);
    Some((idle, fields.iter().sum()))
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn thread_work() -> Option<u64> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn system_times() -> Option<(u64, u64)> {
    None
}

/// The rate of [`thread_work`] units per millisecond, measured by spinning this thread: the
/// highest of several short spins (a spin the thread was taken off its core for reads low).
pub fn work_per_ms() -> Option<f64> {
    let mut best: Option<f64> = None;
    for _ in 0..12 {
        let w0 = thread_work()?;
        let t0 = std::time::Instant::now();
        let mut x = 0u64;
        while t0.elapsed() < std::time::Duration::from_micros(1500) {
            x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let w = thread_work()?.saturating_sub(w0) as f64;
        if ms > 0.0 && w > 0.0 {
            best = Some(best.map_or(w / ms, |b: f64| b.max(w / ms)));
        }
    }
    best
}

/// Share of the machine's CPU time that was busy between two [`system_times`] readings.
pub fn busy_between(before: Option<(u64, u64)>, after: Option<(u64, u64)>) -> Option<f32> {
    before.zip(after).and_then(|((i0, t0), (i1, t1))| {
        let total = t1.saturating_sub(t0);
        (total > 0).then(|| 1.0 - i1.saturating_sub(i0) as f32 / total as f32)
    })
}
