//! "Win" as the hotkey: a tap of the Windows key alone opens Lightning
//! instead of the Start menu, while Win+<key> shortcuts keep working.
//!
//! RegisterHotKey can't bind a lone modifier, so this uses a low-level
//! keyboard hook on its own thread (it must answer within Windows' hook
//! timeout regardless of what the UI thread is doing). It only tracks key
//! codes: whether anything else was pressed while Win was held. It is
//! installed only while "Win" is the hotkey or the settings recorder waits
//! for a key.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, MSG, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Unassigned virtual key, injected so Windows sees "Win+something" and
/// skips the Start menu. Nothing reacts to it.
const MASK_VK: u16 = 0xE8;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

static TARGET: AtomicIsize = AtomicIsize::new(0);
static MESSAGE: AtomicU32 = AtomicU32::new(0);
static THREAD: AtomicU32 = AtomicU32::new(0);
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static OTHER_KEY: AtomicBool = AtomicBool::new(false);

/// Installs the hook (once). Each lone Win tap posts `message` to `hwnd`.
pub fn enable(hwnd: HWND, message: u32) {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    MESSAGE.store(message, Ordering::Relaxed);
    if THREAD.load(Ordering::Relaxed) != 0 {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("winkey-hook".into()).spawn(move || unsafe {
        let hinst = GetModuleHandleW(None).ok();
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hinst.map(Into::into), 0);
        let _ = tx.send(hook.is_ok().then(|| GetCurrentThreadId()));
        let Ok(hook) = hook else { return };
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        let _ = UnhookWindowsHookEx(hook);
    });
    if spawned.is_ok()
        && let Ok(Some(tid)) = rx.recv()
    {
        THREAD.store(tid, Ordering::Relaxed);
    }
}

pub fn disable() {
    let tid = THREAD.swap(0, Ordering::Relaxed);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
    WIN_DOWN.store(false, Ordering::Relaxed);
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let k = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
        let is_win = k.vkCode == VK_LWIN || k.vkCode == VK_RWIN;
        match wp.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN if is_win => {
                // Auto-repeat while held must not reset "something else was pressed".
                if !WIN_DOWN.swap(true, Ordering::Relaxed) {
                    OTHER_KEY.store(false, Ordering::Relaxed);
                }
            }
            WM_KEYDOWN | WM_SYSKEYDOWN if k.vkCode != MASK_VK as u32 => {
                if WIN_DOWN.load(Ordering::Relaxed) {
                    OTHER_KEY.store(true, Ordering::Relaxed);
                }
            }
            WM_KEYUP | WM_SYSKEYUP if is_win => {
                if WIN_DOWN.swap(false, Ordering::Relaxed) && !OTHER_KEY.load(Ordering::Relaxed) {
                    // Before Windows sees this key-up: turn the tap into a
                    // "chord" so Start stays closed. Injecting input also
                    // lets Lightning take keyboard focus when it opens.
                    send_mask_key();
                    let hwnd = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), MESSAGE.load(Ordering::Relaxed), WPARAM(0), LPARAM(0));
                    }
                }
            }
            _ => {}
        }
    }
    unsafe { CallNextHookEx(None::<HHOOK>, code, wp, lp) }
}

fn send_mask_key() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(MASK_VK), dwFlags: flags, ..Default::default() },
        },
    };
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}
