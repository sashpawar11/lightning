//! File search backed by fff.
//!
//! One fff picker per indexed root. Drive roots themselves are not indexed
//! directly: we split each fixed drive into its top-level folders and skip
//! the OS-owned ones (`Windows`, `Program Files`, ...), which would otherwise
//! dominate both memory and results with files nobody launches by name.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use fff_search::file_picker::FilePicker;
use fff_search::{
    FFFQuery, FilePickerOptions, FileSearchConfig, FuzzySearchOptions, PaginationArgs, QueryParser, SharedFilePicker,
    SharedFrecency,
};

use crate::fuzzy::Ranges;

/// Top-level folders never worth indexing (compared case-insensitively).
const SKIP_TOP_LEVEL: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "$winreagent",
    "$windows.~bt",
    "$windows.~ws",
    "system volume information",
    "recovery",
    "perflogs",
    "msocache",
    "config.msi",
    "windows.old",
    "intel",
    "amd",
    "nvidia",
];

#[derive(Debug, Clone)]
pub struct FileHit {
    pub score: i32,
    pub path: PathBuf,
    pub name: String,
    pub dir: String,
    /// Byte ranges into `name`.
    pub name_ranges: Ranges,
}

struct Root {
    base: PathBuf,
    picker: SharedFilePicker,
    /// Latches once the first scan finished (or the root failed to open);
    /// later watcher-driven rescans don't count as "indexing".
    scanned: Arc<AtomicBool>,
}

/// Roots at least this large get a multi-threaded match.
const BIG_ROOT: usize = 20_000;

impl Root {
    fn file_count(&self) -> usize {
        self.picker.read().ok().and_then(|g| g.as_ref().map(|p| p.live_file_count())).unwrap_or(0)
    }
}

struct Roots {
    roots: Vec<Root>,
    parser: QueryParser<FileSearchConfig>,
}

/// A search key: (query, limit).
type Key = (String, usize);

/// State shared with the background searcher behind `search_timeout`.
#[derive(Default)]
struct Pending {
    /// The search callers want now.
    latest: Option<Key>,
    /// Queued for the searcher.
    want: Option<Key>,
    running: Option<Key>,
    done: Option<(Key, Vec<FileHit>)>,
    /// File count as of the last recount, and whether one is wanted.
    count: usize,
    recount: bool,
    closed: bool,
}

type Searcher = Arc<(Mutex<Pending>, Condvar)>;

pub struct FileIndex {
    roots: Arc<Roots>,
    searcher: Searcher,
}

impl FileIndex {
    /// Starts background scans + watchers for `roots`. Returns immediately;
    /// searches simply see more files as scans progress.
    ///
    /// Roots start one after another: when a dozen fff pickers begin scanning
    /// at the same moment, some scans intermittently never finish. Scanning
    /// is disk-bound, so sequential start costs little. fff's LMDB frecency
    /// database isn't attached either (it made those hangs far likelier); the
    /// engine boosts files from its own launch history instead.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        let roots: Vec<Root> = roots
            .into_iter()
            .map(|base| Root {
                base,
                picker: SharedFilePicker::default(),
                scanned: Arc::new(AtomicBool::new(false)),
            })
            .collect();
        let starts: Vec<_> =
            roots.iter().map(|r| (r.base.clone(), r.picker.clone(), r.scanned.clone())).collect();
        let _ = std::thread::Builder::new().name("index-start".into()).spawn(move || {
            for (base, picker, scanned) in starts {
                let started = FilePicker::new_with_shared_state(
                    picker.clone(),
                    SharedFrecency::default(),
                    FilePickerOptions {
                        base_path: base.to_string_lossy().into_owned(),
                        enable_fs_root_scanning: true,
                        enable_home_dir_scanning: true,
                        ..Default::default()
                    },
                )
                .is_ok();
                if started {
                    picker.wait_for_scan(Duration::from_secs(300));
                } else {
                    scanned.store(true, Ordering::Relaxed);
                }
            }
        });
        let roots = Arc::new(Roots { roots, parser: QueryParser::default() });
        let searcher = Searcher::default();
        let _ = std::thread::Builder::new().name("file-search".into()).spawn({
            let (roots, searcher) = (roots.clone(), searcher.clone());
            move || run_searcher(&roots, &searcher)
        });
        Self { roots, searcher }
    }

    pub fn roots(&self) -> impl Iterator<Item = &Path> {
        self.roots.roots.iter().map(|r| r.base.as_path())
    }

    /// Blocks until every root finished its initial scan (benches/tests).
    pub fn wait_for_scan(&self, timeout: Duration) -> bool {
        let end = std::time::Instant::now() + timeout;
        while self.is_scanning() {
            if std::time::Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        true
    }

    /// True until every root has completed its initial scan.
    pub fn is_scanning(&self) -> bool {
        self.roots.roots.iter().any(|r| {
            if r.scanned.load(Ordering::Relaxed) {
                return false;
            }
            let active = r.picker.read().ok().and_then(|g| g.as_ref().map(|p| p.is_scan_active()));
            if active == Some(false) {
                r.scanned.store(true, Ordering::Relaxed);
            }
            active != Some(false)
        })
    }

    /// (root, files, still scanning) per root, for diagnostics.
    pub fn root_stats(&self) -> Vec<(&Path, usize, bool)> {
        self.roots
            .roots
            .iter()
            .map(|r| {
                let scanning = r.picker.read().ok().and_then(|g| g.as_ref().map(|p| p.is_scan_active()));
                (r.base.as_path(), r.file_count(), scanning.unwrap_or(true))
            })
            .collect()
    }

    pub fn file_count(&self) -> usize {
        self.roots.file_count()
    }

    /// `file_count` as of a moment ago, without waiting on a busy root.
    pub fn file_count_cached(&self) -> usize {
        let (lock, cv) = &*self.searcher;
        let mut p = lock.lock().unwrap();
        p.recount = true;
        cv.notify_all();
        p.count
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<FileHit> {
        self.roots.search(query, limit)
    }

    /// `search`, but gives up after `wait`, returning `None`. A root is
    /// locked while fff applies watcher changes, which on a busy disk can
    /// take seconds; the search then finishes in the background and a later
    /// call with the same query picks up its result.
    pub fn search_timeout(&self, query: &str, limit: usize, wait: Duration) -> Option<Vec<FileHit>> {
        let key = (query.to_owned(), limit);
        let (lock, cv) = &*self.searcher;
        let mut p = lock.lock().unwrap();
        if p.latest.as_ref() != Some(&key) {
            // A result for an earlier request of this query may be stale.
            p.done = None;
            p.latest = Some(key.clone());
        }
        let finished = |p: &Pending| p.done.as_ref().is_some_and(|d| d.0 == key);
        if !finished(&p) && p.running.as_ref() != Some(&key) {
            p.want = Some(key.clone());
            cv.notify_all();
        }
        let (mut p, _) = cv.wait_timeout_while(p, wait, |p| !finished(p)).unwrap();
        if finished(&p) { p.done.take().map(|d| d.1) } else { None }
    }
}

impl Drop for FileIndex {
    fn drop(&mut self) {
        let (lock, cv) = &*self.searcher;
        lock.lock().unwrap().closed = true;
        cv.notify_all();
    }
}

fn run_searcher(roots: &Roots, searcher: &Searcher) {
    let (lock, cv) = &**searcher;
    let mut p = lock.lock().unwrap();
    loop {
        p = cv.wait_while(p, |p| p.want.is_none() && !p.recount && !p.closed).unwrap();
        if p.closed {
            return;
        }
        if let Some(key) = p.want.take() {
            p.running = Some(key.clone());
            drop(p);
            let hits = roots.search(&key.0, key.1);
            p = lock.lock().unwrap();
            p.running = None;
            p.done = Some((key, hits));
            cv.notify_all();
        }
        if std::mem::take(&mut p.recount) {
            drop(p);
            let count = roots.file_count();
            p = lock.lock().unwrap();
            p.count = count;
        }
    }
}

impl Roots {
    fn file_count(&self) -> usize {
        self.roots.iter().map(Root::file_count).sum()
    }

    fn search(&self, query: &str, limit: usize) -> Vec<FileHit> {
        let query = query.trim();
        if query.is_empty() || limit == 0 || self.roots.is_empty() {
            return Vec::new();
        }
        let parsed = self.parser.parse(query);
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        // Large roots use fff's own parallel matcher. Small roots are searched
        // concurrently, one thread each: a per-picker thread pool would cost
        // more to start than the match itself.
        let (big, small): (Vec<&Root>, Vec<&Root>) =
            self.roots.iter().partition(|r| r.file_count() >= BIG_ROOT);
        let mut hits = Vec::with_capacity(limit * 4);
        for root in big {
            search_root(root, &parsed, limit, cores, &mut hits);
        }
        let next = AtomicUsize::new(0);
        let threads = cores.min(small.len());
        std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(|| {
                        let mut local = Vec::new();
                        while let Some(root) = small.get(next.fetch_add(1, Ordering::Relaxed)) {
                            search_root(root, &parsed, limit, 1, &mut local);
                        }
                        local
                    })
                })
                .collect();
            for w in workers {
                hits.extend(w.join().unwrap_or_default());
            }
        });
        hits.sort_unstable_by(|a, b| b.score.cmp(&a.score));
        hits.truncate(limit);
        hits
    }

}

fn search_root(root: &Root, query: &FFFQuery<'_>, limit: usize, threads: usize, out: &mut Vec<FileHit>) {
    let Ok(guard) = root.picker.read() else { return };
    let Some(picker) = guard.as_ref() else { return };
    let res = picker.fuzzy_search(
        query,
        None,
        FuzzySearchOptions {
            max_threads: threads,
            pagination: PaginationArgs { offset: 0, limit },
            ..Default::default()
        },
    );
    for (i, item) in res.items.iter().enumerate() {
        let rel = item.relative_path(picker);
        let fname_off = item.filename_offset_in_relative_path().min(rel.len());
        let name = rel[fname_off..].to_owned();
        let path = native_path(item.absolute_path(picker, &root.base));
        let dir = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let fo = fname_off as u32;
        let name_ranges = res
            .match_byte_offsets
            .get(i)
            .map(|v| {
                v.iter()
                    .filter(|&&(_, e)| e > fo)
                    .map(|&(s, e)| (s.max(fo) - fo, e - fo))
                    .filter(|&(s, e)| s < e && (e as usize) <= name.len())
                    .collect()
            })
            .unwrap_or_default();
        out.push(FileHit { score: res.scores[i].total, path, name, dir, name_ranges });
    }
}

/// fff joins relative paths with `/`; show and open them with `\` on Windows.
fn native_path(p: PathBuf) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(p.to_string_lossy().replace('/', "\\"))
    } else {
        p
    }
}

/// The default roots: the non-system top-level folders of every fixed drive.
#[cfg(windows)]
pub fn default_roots() -> Vec<PathBuf> {
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, GetDriveTypeW, GetLogicalDrives,
    };
    use windows::core::HSTRING;
    const DRIVE_FIXED: u32 = 3;

    let mask = unsafe { GetLogicalDrives() };
    let mut roots = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let drive = format!("{}:\\", (b'A' + i as u8) as char);
        if unsafe { GetDriveTypeW(&HSTRING::from(drive.as_str())) } != DRIVE_FIXED {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&drive) else { continue };
        let mut keep = Vec::new();
        let mut has_system_dirs = false;
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_dir() {
                continue;
            }
            use std::os::windows::fs::MetadataExt;
            let attrs = meta.file_attributes();
            if attrs & (FILE_ATTRIBUTE_HIDDEN.0 | FILE_ATTRIBUTE_SYSTEM.0) != 0 {
                continue; // fff's walker skips these too
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if SKIP_TOP_LEVEL.contains(&name.as_str()) {
                has_system_dirs = true;
            } else if !name.starts_with('.') {
                keep.push(e.path());
            }
        }
        // One picker per drive where possible: fewer watchers and threads.
        if has_system_dirs {
            roots.extend(keep);
        } else {
            roots.push(PathBuf::from(drive));
        }
    }
    roots
}

#[cfg(not(windows))]
pub fn default_roots() -> Vec<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn busy_root_does_not_block_search() {
        let dir = std::env::temp_dir().join(format!("lightning-files-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("quarterly_report.txt"), b"x").unwrap();
        let idx = FileIndex::new(vec![dir.clone()]);
        assert!(idx.wait_for_scan(Duration::from_secs(30)));

        // What fff's watcher does while it applies a burst of changes.
        let picker = idx.roots.roots[0].picker.clone();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _guard = picker.write().unwrap();
            locked_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        locked_rx.recv().unwrap();

        let t = Instant::now();
        assert!(idx.search_timeout("report", 6, Duration::from_millis(50)).is_none());
        assert!(t.elapsed() < Duration::from_secs(1));

        release_tx.send(()).unwrap();
        holder.join().unwrap();
        let hits = idx.search_timeout("report", 6, Duration::from_secs(5)).unwrap();
        assert_eq!(hits[0].name, "quarterly_report.txt");
        // A new search for another query doesn't get the old result.
        assert!(idx.search_timeout("nothing-here", 6, Duration::from_secs(5)).unwrap().is_empty());
        drop(idx);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
