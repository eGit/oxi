//! Keep the computer from idle-sleeping while an agent works.
//!
//! A long unattended turn otherwise stops when the laptop sleeps: the agent subprocess freezes and
//! its provider stream times out. Only *idle* sleep is held off. The display may still turn off,
//! and closing the lid or picking Sleep still sleeps, as the user asked for it.
//!
//! - macOS: an IOKit `PreventUserIdleSystemSleep` assertion, released by the OS if oxi dies.
//! - Windows: `SetThreadExecutionState(ES_SYSTEM_REQUIRED)` on the calling thread.
//! - Linux: a `systemd-inhibit` child holding a `sleep:idle` lock. It runs `cat` on a pipe from
//!   oxi, so it exits, and the lock goes, as soon as oxi does.

/// Why oxi holds the lock, where the OS shows it (`pmset -g assertions`, `systemd-inhibit --list`).
/// Windows's thread execution state takes no reason.
#[cfg_attr(target_os = "windows", allow(dead_code))]
const REASON: &str = "An oxi agent is working";

/// Holds the sleep lock while [`KeepAwake::set`] was last called with `true`.
#[derive(Default)]
pub struct KeepAwake {
    wanted: bool,
    hold: Option<platform::Hold>,
}

impl KeepAwake {
    /// Cheap to call every frame: the OS is only asked when `on` changes.
    pub fn set(&mut self, on: bool) {
        if on == self.wanted {
            return;
        }
        self.wanted = on;
        // A failed acquire is logged once and not retried until the next run starts.
        self.hold = if on { platform::Hold::acquire() } else { None };
        log::info!(
            "keep awake: {}",
            match (on, self.hold.is_some()) {
                (true, true) => "holding off idle sleep",
                (true, false) => "could not hold off idle sleep",
                (false, _) => "released",
            }
        );
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{CString, c_char, c_void};

    type CFStringRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    const ASSERTION_LEVEL_ON: u32 = 255;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            level: u32,
            name: CFStringRef,
            id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    fn cf_string(text: &str) -> Option<CFStringRef> {
        let text = CString::new(text).ok()?;
        // SAFETY: `text` is a valid NUL-terminated string for the duration of the call.
        let s = unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) };
        (!s.is_null()).then_some(s)
    }

    pub struct Hold(u32);

    impl Hold {
        pub fn acquire() -> Option<Self> {
            let kind = cf_string("PreventUserIdleSystemSleep")?;
            let Some(name) = cf_string(super::REASON) else {
                // SAFETY: `kind` was created above and is released once.
                unsafe { CFRelease(kind) };
                return None;
            };
            let mut id = 0;
            // SAFETY: both strings are live CFStrings and `id` is a valid out pointer.
            let status =
                unsafe { IOPMAssertionCreateWithName(kind, ASSERTION_LEVEL_ON, name, &mut id) };
            // SAFETY: the assertion keeps its own references; ours are released once.
            unsafe {
                CFRelease(kind);
                CFRelease(name);
            }
            if status != 0 {
                log::warn!("keep awake: IOPMAssertionCreateWithName failed ({status:#x})");
                return None;
            }
            Some(Self(id))
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            // SAFETY: `self.0` is an assertion this process created and has not released.
            unsafe { IOPMAssertionRelease(self.0) };
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows_sys::Win32::System::Power::{
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };

    /// The state belongs to the thread that set it, so it is cleared on that thread: the owner
    /// ([`super::KeepAwake`]) lives on the UI thread in the app and on the server loop headless.
    pub struct Hold;

    impl Hold {
        pub fn acquire() -> Option<Self> {
            // SAFETY: plain Win32 call with valid flags.
            let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            if previous == 0 {
                log::warn!("keep awake: SetThreadExecutionState failed");
                return None;
            }
            Some(Self)
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            // SAFETY: plain Win32 call with valid flags.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::process::{Child, Command, Stdio};

    pub struct Hold(Child);

    impl Hold {
        pub fn acquire() -> Option<Self> {
            Command::new("systemd-inhibit")
                .args([
                    "--what=sleep:idle",
                    "--who=oxi",
                    &format!("--why={}", super::REASON),
                    "--mode=block",
                    "cat",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| log::warn!("keep awake: could not start systemd-inhibit: {e}"))
                .ok()
                .map(Self)
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            // Closing the pipe ends `cat` and with it the lock; kill in case it is stuck.
            drop(self.0.stdin.take());
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
mod platform {
    pub struct Hold;

    impl Hold {
        pub fn acquire() -> Option<Self> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KeepAwake;

    #[test]
    fn only_state_changes_reach_the_os() {
        let mut awake = KeepAwake::default();
        awake.set(false);
        assert!(awake.hold.is_none());
        awake.set(true);
        let held = awake.hold.is_some();
        awake.set(true);
        assert_eq!(
            awake.hold.is_some(),
            held,
            "a repeated `true` must keep the same hold"
        );
        awake.set(false);
        assert!(awake.hold.is_none());
    }
}
