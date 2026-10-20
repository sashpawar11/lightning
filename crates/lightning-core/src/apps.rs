//! Installed applications, enumerated from the shell's `AppsFolder`.
//!
//! AppsFolder is what the Start menu's "All apps" list is built from, so it
//! covers classic Win32 shortcuts and packaged (Store/UWP) apps alike, and
//! every entry launches uniformly via `shell:AppsFolder\<id>`.

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    /// AppsFolder parsing name (an AUMID or a known-folder path).
    pub id: String,
}

impl App {
    pub fn launch_target(&self) -> String {
        format!("shell:AppsFolder\\{}", self.id)
    }
}

fn is_noise(app: &App) -> bool {
    const SKIP_EXT: &[&str] = &[".url", ".chm", ".txt", ".pdf", ".html", ".htm", ".rtf", ".ini"];
    let name = app.name.to_ascii_lowercase();
    let id = app.id.to_ascii_lowercase();
    name.starts_with("uninstall")
        || name.contains("uninstaller")
        || name.ends_with("readme")
        || SKIP_EXT.iter().any(|e| id.ends_with(e))
}

/// A Control Panel task ("Edit the system environment variables"), from
/// the shell's All Tasks folder: the same list Windows Search draws on.
#[derive(Debug, Clone)]
pub struct Task {
    pub name: String,
    /// Serialized shell item ID list. These items have no parsing name the
    /// shell accepts back, so the PIDL is what opens them.
    pub pidl: Box<[u8]>,
}

impl Task {
    /// `pidl:<hex>`, understood by the app's launcher.
    pub fn target(&self) -> String {
        let mut s = String::with_capacity(5 + self.pidl.len() * 2);
        s.push_str("pidl:");
        for b in self.pidl.iter() {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}

#[cfg(windows)]
pub fn enumerate() -> Vec<App> {
    use windows::Win32::UI::Shell::{
        FOLDERID_AppsFolder, IShellItem, KF_FLAG_DEFAULT, SHGetKnownFolderItem,
        SIGDN_PARENTRELATIVEPARSING,
    };
    init_com();
    let Ok(folder) =
        (unsafe { SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) })
    else {
        return Vec::new();
    };
    let mut out: Vec<App> = shell_children(&folder, |i| unsafe { display_name(i, SIGDN_PARENTRELATIVEPARSING) })
        .into_iter()
        .map(|(name, id)| App { name, id })
        .filter(|a| !is_noise(a))
        .collect();
    out.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out.shrink_to_fit();
    out
}

#[cfg(windows)]
pub fn enumerate_tasks() -> Vec<Task> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{ILGetSize, IShellItem, SHCreateItemFromParsingName, SHGetIDListFromObject};
    use windows::core::w;
    init_com();
    let Ok(folder) = (unsafe {
        SHCreateItemFromParsingName::<_, _, IShellItem>(
            w!("shell:::{ED7BA470-8E54-465E-825C-99712043E01C}"),
            None,
        )
    }) else {
        return Vec::new();
    };
    let pidl_bytes = |item: &IShellItem| unsafe {
        let p = SHGetIDListFromObject(item).ok()?;
        let bytes = std::slice::from_raw_parts(p as *const u8, ILGetSize(Some(p)) as usize).into();
        CoTaskMemFree(Some(p as *const _));
        Some(bytes)
    };
    let mut out: Vec<Task> = shell_children(&folder, pidl_bytes)
        .into_iter()
        .map(|(name, pidl)| Task { name, pidl })
        .collect();
    drop(folder);
    // Unload the Control Panel shell extensions enumeration pulled in.
    unsafe { windows::Win32::System::Com::CoFreeUnusedLibrariesEx(0, None) };
    out.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out.shrink_to_fit();
    out
}

#[cfg(windows)]
fn init_com() {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};
    // Harmless if this thread already initialized COM.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
}

#[cfg(windows)]
unsafe fn display_name(
    item: &windows::Win32::UI::Shell::IShellItem,
    kind: windows::Win32::UI::Shell::SIGDN,
) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(kind).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// (display name, `id(child)`) for every child of a shell folder.
#[cfg(windows)]
fn shell_children<T>(
    folder: &windows::Win32::UI::Shell::IShellItem,
    id: impl Fn(&windows::Win32::UI::Shell::IShellItem) -> Option<T>,
) -> Vec<(String, T)> {
    use windows::Win32::UI::Shell::{BHID_EnumItems, IEnumShellItems, IShellItem, SIGDN_NORMALDISPLAY};

    let mut out = Vec::with_capacity(256);
    unsafe {
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else {
            return out;
        };
        let mut batch: [Option<IShellItem>; 32] = Default::default();
        loop {
            let mut fetched = 0u32;
            if items.Next(&mut batch, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            for item in batch.iter_mut().take(fetched as usize) {
                let Some(item) = item.take() else { continue };
                if let (Some(n), Some(v)) = (display_name(&item, SIGDN_NORMALDISPLAY), id(&item)) {
                    out.push((n, v));
                }
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn enumerate() -> Vec<App> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn enumerate_tasks() -> Vec<Task> {
    Vec::new()
}
