// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the MIT license. See LICENSE in the repository root.

//! What changes when the Launcher is the machine's init (PID 1).
//!
//! The kernel panics when init exits, so as PID 1 the Launcher never exits.
//! Where it would have exited it stops every remaining process, flushes the
//! filesystems and powers the machine off.

use std::process;

/// Whether this process is the machine's init.
pub fn is_init() -> bool { process::id() == 1 }

/// Ends the Launcher with `code`, or powers the machine off if it is init.
pub fn exit(code: i32) -> ! {
    if is_init() {
        imp::power_off(code)
    }
    process::exit(code)
}

#[cfg(target_os = "linux")]
mod imp {
    use dojo_common::outputln;
    use libc;
    use std::{io,
              ptr,
              thread,
              time::{Duration,
                     Instant}};

    static LOGKEY: &'static str = "IN";

    /// How long the remaining processes get to exit after SIGTERM before they are killed.
    const TERM_GRACE: Duration = Duration::from_secs(10);

    pub fn power_off(code: i32) -> ! {
        outputln!("Launcher is init and finished with code {}; powering off",
                  code);

        // As init, a signal to pid -1 reaches every process but this one.
        unsafe { libc::kill(-1, libc::SIGTERM) };
        let deadline = Instant::now() + TERM_GRACE;
        while children_remain() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(100));
        }
        unsafe { libc::kill(-1, libc::SIGKILL) };
        while unsafe { libc::waitpid(-1, ptr::null_mut(), 0) } > 0 {}

        unsafe {
            libc::sync();
            libc::reboot(libc::RB_POWER_OFF);
        }
        // reboot(2) returns only when it fails, and init still must not exit.
        error!("Unable to power off: {}", io::Error::last_os_error());
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }

    /// Reaps every child that has exited; false once no children are left.
    fn children_remain() -> bool {
        loop {
            match unsafe { libc::waitpid(-1, ptr::null_mut(), libc::WNOHANG) } {
                0 => return true,
                pid if pid > 0 => continue,
                _ => return false,
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::process;

    // Dojo is init only on Linux.
    pub fn power_off(code: i32) -> ! { process::exit(code) }
}
