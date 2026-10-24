//! Performance guardrails. Run `scripts/bench.ps1` to compare against the
//! saved baseline; any regression beyond the noise threshold fails the run.

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use lightning_core::apps::App;
use lightning_core::files::FileIndex;
use lightning_core::fuzzy::Matcher;
use lightning_core::Engine;

const WORDS: &[&str] = &[
    "visual", "studio", "code", "terminal", "report", "quarterly", "notes", "spotify", "player",
    "calc", "editor", "photo", "media", "server", "client", "manager", "office", "draft", "final",
    "invoice", "budget", "design", "system", "monitor", "backup", "sync", "chrome", "browser",
];

fn synthetic_apps(n: usize) -> Vec<App> {
    (0..n)
        .map(|i| {
            let a = WORDS[i % WORDS.len()];
            let b = WORDS[(i * 7 + 3) % WORDS.len()];
            let c = WORDS[(i * 13 + 5) % WORDS.len()];
            let name = format!("{}{} {} {}", a[..1].to_uppercase(), &a[1..], b, c);
            App { id: format!("app.{i}"), name }
        })
        .collect()
}

/// A deterministic ~20k-file tree, created once and reused across runs.
fn corpus() -> PathBuf {
    let root = std::env::temp_dir().join("lightning-bench-corpus-v1");
    let marker = root.join(".complete");
    if marker.exists() {
        return root;
    }
    let _ = std::fs::remove_dir_all(&root);
    for d in 0..200 {
        let dir = root
            .join(WORDS[d % WORDS.len()])
            .join(format!("{}-{d}", WORDS[(d * 5) % WORDS.len()]));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..100 {
            let name = format!(
                "{}_{}_{f}.{}",
                WORDS[(d + f) % WORDS.len()],
                WORDS[(d * 3 + f * 7) % WORDS.len()],
                ["rs", "md", "txt", "pdf", "docx"][f % 5]
            );
            std::fs::write(dir.join(name), b"x").unwrap();
        }
    }
    std::fs::write(&marker, b"").unwrap();
    root
}

fn bench_fuzzy(c: &mut Criterion) {
    let apps = synthetic_apps(400);
    let mut m = Matcher::default();
    let mut g = c.benchmark_group("fuzzy");
    for q in ["v", "vsc", "terminal", "qrtly rep"] {
        g.bench_function(format!("400_apps/{q}"), |b| {
            b.iter(|| {
                m.set_query(black_box(q));
                apps.iter().filter_map(|a| m.score(&a.name)).count()
            })
        });
    }
    g.finish();
}

fn bench_engine_no_files(c: &mut Criterion) {
    let mut e = Engine::new(synthetic_apps(400), None, None);
    // ~200 Control Panel tasks, like a real machine.
    e.set_tasks(
        (0..200)
            .map(|i| lightning_core::apps::Task {
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
    let mut g = c.benchmark_group("engine");
    for q in ["s", "sound", "vis stu", "volumne control", "edit environment variables"] {
        g.bench_function(format!("apps+settings/{q}"), |b| b.iter(|| e.search(black_box(q))));
    }
    g.finish();
}

fn bench_calc(c: &mut Criterion) {
    let mut g = c.benchmark_group("calc");
    g.bench_function("eval", |b| b.iter(|| lightning_core::calc::eval(black_box("(1,250 + 3.5) * 2 / sqrt(16) ^ 2"))));
    // What every ordinary query pays: a word, and a word that gets parsed.
    g.bench_function("reject/chrome", |b| b.iter(|| lightning_core::calc::eval(black_box("chrome"))));
    g.bench_function("reject/7-zip", |b| b.iter(|| lightning_core::calc::eval(black_box("7-zip"))));
    // Every keystroke pays this one.
    g.bench_function("is_math/chrome", |b| b.iter(|| lightning_core::calc::is_math(black_box("chrome"))));
    g.bench_function("percent", |b| b.iter(|| lightning_core::calc::eval(black_box("200 + 10% + 20% of 150"))));
    g.finish();
}

fn bench_power(c: &mut Criterion) {
    let mut g = c.benchmark_group("power");
    // "chrome" is the rejection cost every ordinary query pays.
    for q in ["chrome", "visual studio", "s", "shut", "power"] {
        g.bench_function(q, |b| b.iter(|| lightning_core::power::matches(black_box(q))));
    }
    g.finish();
}

fn bench_files(c: &mut Criterion) {
    let root = corpus();
    let index = FileIndex::new(vec![root]);
    assert!(index.wait_for_scan(Duration::from_secs(60)), "scan timed out");
    let mut g = c.benchmark_group("files_20k");
    for q in ["report", "qrt notes", "budget_final pdf", "x"] {
        g.bench_function(q, |b| b.iter(|| index.search(black_box(q), 6)));
    }
    g.finish();
}

fn bench_index_build(c: &mut Criterion) {
    let root = corpus();
    let mut g = c.benchmark_group("startup");
    g.sample_size(10).measurement_time(Duration::from_secs(15));
    g.bench_function("index_20k_files", |b| {
        b.iter(|| {
            let idx = FileIndex::new(vec![root.clone()]);
            assert!(idx.wait_for_scan(Duration::from_secs(60)));
            idx.file_count()
        })
    });
    #[cfg(windows)]
    g.bench_function("enumerate_installed_apps", |b| {
        b.iter(lightning_core::apps::enumerate)
    });
    g.finish();
}

criterion_group!(benches, bench_fuzzy, bench_engine_no_files, bench_calc, bench_power, bench_files, bench_index_build);
criterion_main!(benches);
