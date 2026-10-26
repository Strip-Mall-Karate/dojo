// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! What changes when the Launcher is the machine's init (PID 1).
//!
//! The kernel panics when init exits, so as PID 1 the Launcher never exits.
//! Where it would have exited it stops every remaining process, flushes the
//! filesystems and powers the machine off, or reboots it.

use std::process;

/// Whether this process is the machine's init.
pub fn is_init() -> bool { process::id() == 1 }

/// Prepares the machine for an init Launcher. The kernel reboots at once on
/// Ctrl-Alt-Del unless init asks for it as SIGINT, which the Launcher turns
/// into a clean reboot.
pub fn prepare() {
    if is_init() {
        imp::catch_ctrl_alt_del()
    }
}

/// Ends the Launcher with `code`, or powers the machine off if it is init.
pub fn exit(code: i32) -> ! {
    if is_init() {
        imp::halt(imp::Halt::PowerOff, code)
    }
    process::exit(code)
}

/// Reboots the machine. A Launcher that isn't init just exits.
pub fn reboot() -> ! {
    if is_init() {
        imp::halt(imp::Halt::Reboot, 0)
    }
    process::exit(0)
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

    pub enum Halt {
        PowerOff,
        Reboot,
    }

    pub fn catch_ctrl_alt_del() {
        if unsafe { libc::reboot(libc::RB_DISABLE_CAD) } != 0 {
            error!("Unable to catch Ctrl-Alt-Del: {}",
                   io::Error::last_os_error());
        }
    }

    pub fn halt(how: Halt, code: i32) -> ! {
        let (verb, cmd) = match how {
            Halt::PowerOff => ("powering off", libc::RB_POWER_OFF),
            Halt::Reboot => ("rebooting", libc::RB_AUTOBOOT),
        };
        outputln!("Launcher is init and finished with code {}; {}", code, verb);

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
            libc::reboot(cmd);
        }
        // reboot(2) returns only when it fails, and init still must not exit.
        error!("Unable to finish {}: {}", verb, io::Error::last_os_error());
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
    pub enum Halt {
        PowerOff,
        Reboot,
    }

    pub fn catch_ctrl_alt_del() {}

    pub fn halt(_how: Halt, code: i32) -> ! { process::exit(code) }
}
