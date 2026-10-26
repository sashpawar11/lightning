# Changelog

## Unreleased

### Fixed
- **No more constant disk reads in the background.** After indexing a drive, and again after every rescan, Lightning read every file under 10 MB from start to end, gigabytes per pass, which also caused CPU and memory spikes. It now only lists file names, so once indexing finishes the disk stays quiet.

## 0.4.0 · 2026-10-22

### New
- **Automatic updates.** Lightning checks for a new version once a day. When there is one, a notification appears; click it, or **Update to X** in the tray menu, and Lightning updates and restarts by itself. **Check for updates** in the tray menu checks right away. Updating from 0.3.0 or earlier still needs this installer once.
- **Right-click menu on results and recents.** Right-click any result or recent tile, or press the menu key or Shift+F10. **Run as administrator** works for classic and Store apps, and **Open file location** shows the file or the app's executable in Explorer.

### Fixed
- **Searches no longer get stuck on old results.** While files were changing in the background, typing could keep showing earlier results for a while. Apps and settings now always answer straight away, and file results follow as soon as they're ready.
- **Much lower background disk use.** Changes inside hidden folders (browser caches, the Recycle Bin, AppData) used to make Lightning re-read whole drives as often as every 30 seconds, and every changed file was read in full. Lightning now ignores hidden folders, never reads file contents, and rescans a drive at most every 5 minutes, or every 15 for very large ones. A burst of thousands of new files can take up to that long to show up in search.
- **The Dark mode tile now changes Lightning's theme too** when it's set to Light or Dark instead of Auto.

## 0.3.0 · 2026-10-20

### New
- **Quick settings.** Tiles in the overlay switch Wi‑Fi, Bluetooth, sound (mute) and dark mode, and show each one's current state.
- **One-click updates.** When an older Lightning is installed, the installer offers **Update now**, which keeps your current options and skips the setup pages, or **Choose options**.
- **Faster results.** Results appear immediately and icons load in behind them; refreshing the app list no longer delays searches.
- Recent items now show up to 12 entries.

### Changed
- The overlay now closes when it loses focus by default. Turn on **Always on top** in settings for the old behaviour.
- Settings opened from the gear closes on Esc or a click away, and has a minimize button.

## 0.2.0 · 2026-10-20

### New
- **Use the Windows key as your hotkey.** In the tray settings, click **Record** and tap **Win**. A tap opens Lightning instead of the Start menu, while Win+E, Win+D, Win+R and other Windows shortcuts keep working. Record any other shortcut to get Start back.

## 0.1.0 · 2026-10-20

First release.
- Search installed apps, files on every fixed drive (powered by [fff](https://github.com/dmtrKovalenko/fff)), Windows Settings and Control Panel tasks.
- Typo-tolerant matching, launch history ranking, real app and file icons.
- Frosted-glass UI with adjustable strength, automatic light/dark theme.
- Tray settings: hotkey, theme, glass, sources, start at sign-in, always on top.
- ~45 MB RAM idle with ~200k files indexed.
