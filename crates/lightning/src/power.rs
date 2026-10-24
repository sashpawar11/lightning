//! Runs power shortcuts. Everything here is a plain Win32 call; the
//! destructive ones are never forced, so apps can still ask to save.
//!
//! Updates: Windows has no documented "skip updates" flag, and none of this
//! touches Windows Update settings. `SHUTDOWN_INSTALL_UPDATES` is the
//! documented opt-in that makes a shutdown install updates, so we never ask
//! for it, and shutdown is a hybrid (fast-startup) one like Start's plain
//! "Shut down". That is not a guarantee: servicing Windows already staged
//! for the next boot, or a restart that completes staged updates, still runs.

use lightning_core::power::Action;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{
    EWX_HYBRID_SHUTDOWN, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, ExitWindowsEx, LockWorkStation, SHTDN_REASON_FLAG_PLANNED,
    SHTDN_REASON_MAJOR_OTHER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::w;

/// Performs `action`. Sleep and hibernate block until the PC wakes, so they
/// get a short-lived thread; the rest return at once.
pub fn run(action: Action) {
    match action {
        Action::Lock => unsafe {
            let _ = LockWorkStation();
        },
        Action::Sleep => suspend(false),
        Action::Hibernate => suspend(true),
        Action::SignOut => exit(EWX_LOGOFF),
        Action::Restart => exit(EWX_REBOOT),
        Action::Shutdown => exit(EWX_POWEROFF | EWX_HYBRID_SHUTDOWN),
    }
}

fn suspend(hibernate: bool) {
    std::thread::spawn(move || unsafe {
        let _ = SetSuspendState(hibernate, false, false);
    });
}

/// `ExitWindowsEx` without `EWX_FORCE`: running apps may veto or save.
fn exit(flags: windows::Win32::System::Shutdown::EXIT_WINDOWS_FLAGS) {
    // Logging off needs no privilege; shutdown and restart do.
    if flags != EWX_LOGOFF {
        enable_shutdown_privilege();
    }
    unsafe {
        let _ = ExitWindowsEx(flags, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED);
    }
}

fn enable_shutdown_privilege() {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token).is_err() {
            return;
        }
        let mut tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, ..Default::default() };
        tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(None, w!("SeShutdownPrivilege"), &mut luid).is_ok() {
            tp.Privileges[0].Luid = luid;
            let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        }
        let _ = CloseHandle(token);
    }
}
