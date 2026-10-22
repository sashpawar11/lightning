use crate::shared::{SharedFrecency, WeakFilePicker};
use ahash::AHashSet;
use parking_lot::{Condvar, Mutex};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// we don't really need a queue here
#[derive(Default)]
struct Pending {
    paths: AHashSet<PathBuf>,
    full_rescan: bool,
    shutdown: bool,
}

impl Pending {
    fn has_work(&self) -> bool {
        self.full_rescan || !self.paths.is_empty()
    }
}

/// Condvar based queue that is used for batch processing events
pub(crate) struct GitStatusWorker {
    state: Mutex<Pending>,
    cv: Condvar,
    consumer_spawned: AtomicBool,
}

impl GitStatusWorker {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(Pending::default()),
            cv: Condvar::new(),
            consumer_spawned: AtomicBool::new(false),
        })
    }

    pub(crate) fn spawn_once(
        self: &Arc<Self>,
        weak_picker: WeakFilePicker,
        frecency: SharedFrecency,
    ) {
        if self
            .consumer_spawned
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Self::spawn_consumer(Arc::clone(self), weak_picker, frecency);
        }
    }

    pub(crate) fn enqueue_paths<I>(&self, paths: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut guard = self.state.lock();
        guard.paths.extend(paths);
        drop(guard);
        self.cv.notify_one();
    }

    pub(crate) fn request_full_rescan(&self) {
        let mut guard = self.state.lock();
        guard.full_rescan = true;
        drop(guard);
        self.cv.notify_one();
    }

    pub(crate) fn signal_shutdown(&self) {
        let mut guard = self.state.lock();
        guard.shutdown = true;
        drop(guard);
        self.cv.notify_one();
    }

    fn wait_and_take(&self) -> Option<Pending> {
        let mut guard = self.state.lock();
        while !guard.shutdown && !guard.has_work() {
            self.cv.wait(&mut guard);
        }
        if guard.shutdown {
            return None;
        }
        Some(std::mem::take(&mut *guard))
    }

    // the problem: git status update can take a lot of time especially on big repositories
    // and there is unpredictable wait time on the lock file if huge commit is going so we have to
    // spawn a separate thread to guartee that notify handler is unlocked even if git update takes a
    // lot of time on every event burst (pretty cheap as this thread is going to sleep 99.9% of time)
    fn spawn_consumer(
        mailbox: Arc<GitStatusWorker>,
        weak_picker: WeakFilePicker,
        frecency: SharedFrecency,
    ) {
        let _ = std::thread::Builder::new()
            .name("fff-git-status".into())
            .spawn(move || {
                while let Some(work) = mailbox.wait_and_take() {
                    let Some(picker) = weak_picker.upgrade() else {
                        break;
                    };

                    if work.full_rescan {
                        if let Err(e) = picker.refresh_git_status(&frecency) {
                            tracing::error!("git-status worker: full rescan failed: {e:?}");
                        }
                    } else if !work.paths.is_empty() {
                        let paths: Vec<PathBuf> = work.paths.into_iter().collect();
                        if let Err(e) = picker.update_git_status_for_paths(&paths, &frecency) {
                            tracing::error!("git-status worker: path update failed: {e:?}");
                        }
                    }
                }

                tracing::info!("git-status worker stopped");
            })
            .inspect_err(|err| tracing::error!(?err, "Failed to spawn git status worker"));
    }
}
