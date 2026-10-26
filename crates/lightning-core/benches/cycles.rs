//! Noise-resistant benchmark: CPU cycles charged to this thread only
//! (`QueryThreadCycleTime`), so time spent preempted by other processes is
//! not counted, scaled to core cycles by a calibration loop so CPU frequency
//! changes cancel out. The thread is pinned to one core at high priority, and each
//! case reports the median of many paired samples. Allocations per call are counted exactly.
//!
//! `cargo bench -p lightning-core --bench cycles` prints the table.
//! `LIGHTNING_BENCH_SAVE=file` writes the results; `LIGHTNING_BENCH_BASELINE=file`
//! compares against a saved run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use lightning_core::Engine;
use lightning_core::apps::{App, Task};

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const WORDS: &[&str] = &[
    "visual", "studio", "code", "terminal", "report", "quarterly", "notes", "spotify", "player",
    "calc", "editor", "photo", "media", "server", "client", "manager", "office", "draft", "final",
    "invoice", "budget", "design", "system", "monitor", "backup", "sync", "chrome", "browser",
];

/// Same synthetic machine as `search.rs`: 400 apps, 200 Control Panel tasks.
fn engine() -> Engine {
    let apps = (0..400)
        .map(|i| {
            let a = WORDS[i % WORDS.len()];
            let b = WORDS[(i * 7 + 3) % WORDS.len()];
            let c = WORDS[(i * 13 + 5) % WORDS.len()];
            App { id: format!("app.{i}"), name: format!("{}{} {b} {c}", a[..1].to_uppercase(), &a[1..]) }
        })
        .collect();
    let mut e = Engine::new(apps, None, None);
    e.set_tasks(
        (0..200)
            .map(|i| Task {
                name: format!(
                    "{} the {} {} settings",
                    ["Change", "Edit", "Adjust", "View", "Set up"][i % 5],
                    WORDS[(i * 3) % WORDS.len()],
                    WORDS[(i * 11 + 1) % WORDS.len()]
                ),
                pidl: Box::new([i as u8]),
            })
            .collect(),
    );
    e
}

fn thread_cycles() -> u64 {
    use windows::Win32::System::Threading::GetCurrentThread;
    use windows::Win32::System::WindowsProgramming::QueryThreadCycleTime;
    let mut c = 0;
    unsafe {
        let _ = QueryThreadCycleTime(GetCurrentThread(), &mut c);
    }
    c
}

fn isolate() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, HIGH_PRIORITY_CLASS, SetPriorityClass, SetThreadAffinityMask,
        SetThreadPriority, THREAD_PRIORITY_HIGHEST,
    };
    unsafe {
        // Core 2: away from core 0, where most interrupts land.
        SetThreadAffinityMask(GetCurrentThread(), 1 << 2);
        let _ = SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }
}

struct Stat {
    name: String,
    min: f64,
    median: f64,
    allocs: f64,
}

/// Dependent 64-bit multiplies: 3 core cycles each on current x86 cores.
const CALIB_MULS: u64 = 20_000;

/// In asm so the compiler can't reassociate the chain into fewer multiplies.
fn calibration() -> u64 {
    let mut x = black_box(0x9E37_79B9_7F4A_7C15u64);
    let c = black_box(0x2545_F491_4F6C_DD1Du64);
    for _ in 0..CALIB_MULS / 10 {
        unsafe {
            std::arch::asm!(
                "imul {x}, {c}", "imul {x}, {c}", "imul {x}, {c}", "imul {x}, {c}", "imul {x}, {c}",
                "imul {x}, {c}", "imul {x}, {c}", "imul {x}, {c}", "imul {x}, {c}", "imul {x}, {c}",
                x = inout(reg) x, c = in(reg) c, options(pure, nomem, nostack),
            );
        }
    }
    black_box(x)
}

/// Core cycles per call. Thread cycle time ticks at the fixed TSC rate, so
/// each sample is paired with a calibration run and scaled by it: turbo and
/// thermal throttling then cancel out. 300 samples of ~200k cycles; the
/// median is the headline figure, the minimum a sanity check.
fn measure(name: &str, mut f: impl FnMut()) -> Stat {
    for _ in 0..1000 {
        f();
    }
    let t = thread_cycles();
    f();
    let one = (thread_cycles() - t).max(1);
    let batch = (200_000 / one).clamp(1, 100_000);
    let a = ALLOCS.load(Relaxed);
    f();
    let allocs = (ALLOCS.load(Relaxed) - a) as f64;
    let mut samples: Vec<f64> = (0..300)
        .map(|_| {
            let t = thread_cycles();
            for _ in 0..batch {
                f();
            }
            let case = thread_cycles() - t;
            let t = thread_cycles();
            calibration();
            let calib = thread_cycles() - t;
            case as f64 / batch as f64 * (3 * CALIB_MULS) as f64 / calib as f64
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    Stat { name: name.into(), min: samples[0], median: samples[samples.len() / 2], allocs }
}

/// Processes per run; each has its own heap and code layout (ASLR), which
/// moves some cases by 20%+, so the result is the median across processes.
const PROCESSES: usize = 7;

fn main() {
    // `cargo bench` passes `--bench`; `cargo test --benches` passes nothing: skip then.
    if !std::env::args().any(|a| a == "--bench") {
        return;
    }
    if std::env::var_os("LIGHTNING_BENCH_CHILD").is_some() {
        isolate();
        for s in cases() {
            println!("{}\t{}\t{}\t{}", s.name, s.min, s.median, s.allocs);
        }
        return;
    }
    let exe = std::env::current_exe().expect("own path");
    let runs: Vec<Vec<Stat>> = (0..PROCESSES)
        .map(|_| {
            let out = std::process::Command::new(&exe)
                .arg("--bench")
                .env("LIGHTNING_BENCH_CHILD", "1")
                .output()
                .expect("run child");
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split('\t').collect();
                    Some(Stat {
                        name: f.first()?.to_string(),
                        min: f.get(1)?.parse().ok()?,
                        median: f.get(2)?.parse().ok()?,
                        allocs: f.get(3)?.parse().ok()?,
                    })
                })
                .collect()
        })
        .collect();
    report(&runs);
}

/// Median across processes per case, its spread (max-min over median), and
/// the change against `LIGHTNING_BENCH_BASELINE`.
fn report(runs: &[Vec<Stat>]) {
    let base: Vec<(String, f64)> = std::env::var("LIGHTNING_BENCH_BASELINE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let mut it = l.split('\t');
                    Some((it.next()?.to_owned(), it.next()?.parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    println!("{} processes; core cycles per call", runs.len());
    println!("{:<48} {:>10} {:>8} {:>7} {:>9}", "case", "median", "spread", "allocs", "vs base");
    let mut saved = String::new();
    for (i, first) in runs[0].iter().enumerate() {
        let mut m: Vec<f64> = runs.iter().filter_map(|r| r.get(i)).map(|s| s.median).collect();
        m.sort_by(f64::total_cmp);
        let median = m[m.len() / 2];
        let spread = (m[m.len() - 1] - m[0]) / median * 100.0;
        let delta = base
            .iter()
            .find(|(n, _)| *n == first.name)
            .map_or(String::new(), |(_, b)| format!("{:+.1}%", (median / b - 1.0) * 100.0));
        println!("{:<48} {:>10.0} {:>7.1}% {:>7} {:>9}", first.name, median, spread, first.allocs, delta);
        saved += &format!("{}\t{}\t{}\t{}\n", first.name, median, spread, first.allocs);
    }
    if let Ok(p) = std::env::var("LIGHTNING_BENCH_SAVE") {
        std::fs::write(p, saved).expect("write results");
    }
}

fn cases() -> Vec<Stat> {
    let mut e = engine();
    let mut out = Vec::new();
    // What every ordinary query pays.
    for q in ["s", "sound", "vis stu", "chrome", "7-zip", "2024-10-24", "edit environment variables"] {
        out.push(measure(&format!("engine/{q}"), || {
            black_box(e.search(black_box(q)));
        }));
    }
    // FEATURES-BEGIN (stripped when benchmarking a build without them)
    for q in ["5+2", "355+(", "sqrt(16)^2", "lock", "pow", "shut"] {
        out.push(measure(&format!("engine/{q}"), || {
            black_box(e.search(black_box(q)));
        }));
    }
    use lightning_core::{calc, power};
    for q in ["chrome", "7-zip", "2024-10-24", "visual studio code"] {
        out.push(measure(&format!("calc::is_math/{q}"), || {
            black_box(calc::is_math(black_box(q)));
        }));
        out.push(measure(&format!("calc::eval/{q}"), || {
            black_box(calc::eval(black_box(q)));
        }));
        out.push(measure(&format!("power::matches/{q}"), || {
            black_box(power::matches(black_box(q)));
        }));
    }
    for q in ["(1,250 + 3.5) * 2 / sqrt(16) ^ 2", "200 + 10% + 20% of 150"] {
        out.push(measure(&format!("calc::eval/{q}"), || {
            black_box(calc::eval(black_box(q)));
        }));
    }
    for q in ["shut", "power"] {
        out.push(measure(&format!("power::matches/{q}"), || {
            black_box(power::matches(black_box(q)));
        }));
    }
    // FEATURES-END

    out
}
