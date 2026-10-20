//! Real-machine probe: indexes the default roots (or the paths given as
//! arguments) and reports timings, counts and memory.
//!
//! cargo run --release -p lightning-core --example probe [-- PATH...]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lightning_core::files::{FileIndex, default_roots};

fn private_mb() -> f64 {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        );
    }
    c.PrivateUsage as f64 / (1024.0 * 1024.0)
}

fn main() {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let roots = if args.is_empty() { default_roots() } else { args };
    println!("roots: {}", roots.len());
    for r in &roots {
        println!("  {}", r.display());
    }
    println!("baseline private: {:.1} MB", private_mb());

    let t = Instant::now();
    let apps = lightning_core::apps::enumerate();
    println!("apps: {} in {:?}, private {:.1} MB", apps.len(), t.elapsed(), private_mb());

    let t = Instant::now();
    let tasks = lightning_core::apps::enumerate_tasks();
    println!("tasks: {} in {:?}, private {:.1} MB", tasks.len(), t.elapsed(), private_mb());
    if std::env::var_os("PROBE_TASKS").is_some() {
        for t in &tasks {
            println!("  {} => {}", t.name, t.target());
        }
        return;
    }

    let t = Instant::now();
    let idx = FileIndex::new(roots);
    if std::env::var_os("PROBE_POLL").is_some() {
        for sec in 0..40 {
            std::thread::sleep(Duration::from_secs(1));
            let busy: Vec<String> = idx
                .root_stats()
                .into_iter()
                .filter(|s| s.2)
                .map(|s| format!("{}({})", s.0.display(), s.1))
                .collect();
            println!("{sec:>2}s files={} scanning: {}", idx.file_count(), busy.join(" "));
        }
        return;
    }
    let done = idx.wait_for_scan(Duration::from_secs(60));
    println!("scan finished: {done}, still scanning: {}", idx.is_scanning());
    for (root, count, scanning) in idx.root_stats() {
        println!("  {:>8} {} {}", count, if scanning { "SCANNING" } else { "done    " }, root.display());
    }
    println!(
        "files: {} in {:?}, private {:.1} MB",
        idx.file_count(),
        t.elapsed(),
        private_mb()
    );
    std::thread::sleep(Duration::from_secs(5));
    println!("after settle: private {:.1} MB", private_mb());

    for q in ["readme", "main rs", "invoice pdf", "cargo toml", "x"] {
        let mut times: Vec<Duration> = (0..21)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(idx.search(q, 6));
                t.elapsed()
            })
            .collect();
        times.sort();
        let hits = idx.search(q, 6);
        let first = hits.first().map(|h| h.path.display().to_string()).unwrap_or_default();
        println!("search {q:>12}: median {:>8.2?}  min {:>8.2?}  top: {first}", times[10], times[0]);
    }
}
