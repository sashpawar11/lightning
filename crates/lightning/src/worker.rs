//! Search thread. Owns the engine and the icon cache so the UI thread only
//! ever paints. Bursts of keystrokes are coalesced to the latest query.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use lightning_core::files::{FileIndex, default_roots};
use lightning_core::{Engine, Hit, Kind, Results, Sources, apps};
use windows::Win32::Foundation::{HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::HSTRING;

use crate::WM_APP_RESULTS;

pub enum Req {
    Query { generation: u64, query: String },
    Launched(Hit),
    Sources(Sources),
    RefreshApps,
}

pub struct Icon {
    pub size: u32,
    /// Premultiplied BGRA, top-down.
    pub px: Vec<u8>,
}

pub struct Resp {
    pub generation: u64,
    pub results: Results,
    pub icons: Vec<Option<Arc<Icon>>>,
    pub file_count: usize,
    pub scanning: bool,
}

const ICON_PX: i32 = 48;

pub fn spawn(hwnd: HWND, data_dir: PathBuf, sources: Sources) -> Sender<Req> {
    let (tx, rx) = mpsc::channel();
    let hwnd = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("search".into())
        .spawn(move || run(rx, HWND(hwnd as *mut _), data_dir, sources))
        .expect("spawn search thread");
    tx
}

fn run(rx: Receiver<Req>, hwnd: HWND, data_dir: PathBuf, sources: Sources) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    // Frecency databases from earlier builds; ranking now uses launch history.
    let _ = std::fs::remove_dir_all(data_dir.join("frecency"));
    let files = FileIndex::new(default_roots());
    let mut engine = Engine::new(apps::enumerate(), Some(files), Some(&data_dir));
    engine.set_tasks(apps::enumerate_tasks());
    engine.sources = sources;
    let mut icons: HashMap<String, Option<Arc<Icon>>> = HashMap::new();
    let mut last: Option<(u64, String)> = None;
    let mut apps_loaded = Instant::now();

    loop {
        let scanning = engine.files().is_some_and(|f| f.is_scanning());
        // While indexing, refresh the visible results as more files land.
        let req = if scanning && last.is_some() {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok(r) => Some(r),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(r) => Some(r),
                Err(_) => return,
            }
        };

        let mut pending = req.into_iter().collect::<Vec<_>>();
        pending.extend(rx.try_iter());
        let mut query = None;
        let mut refresh_apps = false;
        for r in pending {
            match r {
                Req::Query { generation, query: q } => query = Some((generation, q)),
                Req::Launched(hit) => engine.record_launch(&hit),
                Req::Sources(s) => engine.sources = s,
                Req::RefreshApps => refresh_apps = true,
            }
        }
        if let Some(q) = query {
            last = Some(q);
        }
        let Some((generation, q)) = &last else { continue };

        let results = engine.search(q);
        if icons.len() > 512 {
            icons.clear();
        }
        let icon_list = results
            .hits
            .iter()
            .map(|h| {
                let key = icon_key(h)?;
                icons.entry(key).or_insert_with(|| extract_icon(&h.target).map(Arc::new)).clone()
            })
            .collect();
        let (file_count, scanning) = engine
            .files()
            .map_or((0, false), |f| (f.file_count(), f.is_scanning()));
        let resp = Box::new(Resp {
            generation: *generation,
            results,
            icons: icon_list,
            file_count: file_count + engine.app_count(),
            scanning,
        });
        let ptr = Box::into_raw(resp);
        unsafe {
            if PostMessageW(Some(hwnd), WM_APP_RESULTS, WPARAM(0), LPARAM(ptr as isize)).is_err() {
                drop(Box::from_raw(ptr));
            }
        }
        // Pick up newly installed apps, after the user already has results.
        if refresh_apps && apps_loaded.elapsed() > Duration::from_secs(60) {
            engine.set_apps(apps::enumerate());
            apps_loaded = Instant::now();
        }
    }
}

/// Files share one icon per extension, except types whose icon is per file.
fn icon_key(h: &Hit) -> Option<String> {
    match h.kind {
        Kind::Setting => None,
        Kind::App => Some(h.target.clone()),
        Kind::File => {
            let ext = std::path::Path::new(&h.target)
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            match ext.as_deref() {
                None | Some("exe" | "lnk" | "ico" | "url" | "appref-ms" | "msc") => Some(h.target.clone()),
                Some(e) => Some(format!("ext:{e}")),
            }
        }
    }
}

fn extract_icon(target: &str) -> Option<Icon> {
    unsafe {
        let f: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(target), None).ok()?;
        let hbmp: HBITMAP = f.GetImage(SIZE { cx: ICON_PX, cy: ICON_PX }, SIIGBF_ICONONLY).ok()?;
        let icon = read_bitmap(hbmp);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        icon
    }
}

unsafe fn read_bitmap(hbmp: HBITMAP) -> Option<Icon> {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(hbmp.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || h <= 0 || w != h {
            return None;
        }
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        if lines == 0 {
            return None;
        }
        // Shell bitmaps are usually straight alpha; premultiply unless any
        // channel already exceeds its alpha (which only premultiplied can't do).
        let straight = px.chunks_exact(4).any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3]);
        if straight {
            for p in px.chunks_exact_mut(4) {
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = ((*c as u32 * a + 127) / 255) as u8;
                }
            }
        }
        Some(Icon { size: w as u32, px })
    }
}
