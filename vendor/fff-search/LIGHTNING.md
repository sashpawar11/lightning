# fff-search 0.11.0, patched for Lightning

Copied from crates.io (tests and benches left out). Every change is marked
with a `lightning:` comment:

- `src/watcher/background_watcher.rs`, `src/ignore.rs`: the watcher drops
  events under hidden paths (dot names, Windows hidden attribute), which the
  walker never indexes. Before, churn in caches or the recycle bin filled the
  1024-file overflow index and forced full rescans of the root.
- `src/file_picker.rs`: no binary sniffing of changed files. It read each
  created or modified file to the end while holding the picker write lock,
  so large downloads caused heavy disk reads and blocked searches.
- `src/constants.rs`: watcher-triggered full rescans at most every 5 min
  (15 min above 250k files), down from every 30 s.
