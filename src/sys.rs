// The few POSIX calls std does not offer, declared directly (no libc crate):
// pipes at fixed descriptors and the parent-death signal for Chromium and the
// ffmpeg of the overlay, signals taken by a thread instead of a handler, and
// the monotonic clock as a number (Chromium's frame times).
// Linux only, as the images the sidecar and the overlay run in.

use std::ffi::c_int;
use std::io;
use std::os::fd::RawFd;

const SIG_BLOCK: c_int = 0;
const SIG_SETMASK: c_int = 2;
pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;
pub const SIGQUIT: c_int = 3;
const SIGKILL: c_int = 9;
pub const SIGTERM: c_int = 15;
const PR_SET_PDEATHSIG: c_int = 1;
const F_SETFD: c_int = 2;
const CLOCK_MONOTONIC: c_int = 1;
const SOL_SOCKET: c_int = 1;
const SO_KEEPALIVE: c_int = 9;
const IPPROTO_TCP: c_int = 6;
const TCP_KEEPIDLE: c_int = 4;
const TCP_KEEPINTVL: c_int = 5;
const TCP_KEEPCNT: c_int = 6;

/// `struct timespec` of 64-bit Linux (x86_64, aarch64; glibc and musl).
#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

/// `struct tm` of glibc and musl.
#[repr(C)]
struct Tm {
    sec: c_int,
    min: c_int,
    hour: c_int,
    mday: c_int,
    mon: c_int,
    year: c_int,
    wday: c_int,
    yday: c_int,
    isdst: c_int,
    gmtoff: std::os::raw::c_long,
    zone: *const std::os::raw::c_char,
}

/// `sigset_t` of glibc and musl: 1024 bits.
#[repr(C)]
struct SigSet([u64; 16]);

unsafe extern "C" {
    fn dup2(old: c_int, new: c_int) -> c_int;
    fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_int, length: u32) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn kill(pid: c_int, signal: c_int) -> c_int;
    fn prctl(option: c_int, ...) -> c_int;
    fn sigemptyset(set: *mut SigSet) -> c_int;
    fn sigaddset(set: *mut SigSet, signal: c_int) -> c_int;
    fn sigprocmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
    fn pthread_sigmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
    fn sigwait(set: *const SigSet, signal: *mut c_int) -> c_int;
    fn clock_gettime(clock: c_int, time: *mut Timespec) -> c_int;
    fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
}

/// Wall clock `hh:mm:ss` of an epoch time in the process's time zone (`TZ`,
/// e.g. Europe/Warsaw in the stack; UTC without it).
pub fn local_clock(epoch_ms: i64) -> Option<String> {
    let seconds = epoch_ms.div_euclid(1000);
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    // SAFETY: both pointers are valid for the call; `localtime_r` is the reentrant form.
    let result = unsafe { localtime_r(&seconds, &mut tm) };
    (!result.is_null()).then(|| format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec))
}

/// A TCP connection whose peer vanished without closing it (its container gone
/// with data still queued) fails after `idle` + `interval` * `count` seconds
/// instead of blocking the reader for ever.
pub fn keepalive(fd: RawFd, idle: c_int, interval: c_int, count: c_int) -> io::Result<()> {
    for (level, name, value) in [
        (SOL_SOCKET, SO_KEEPALIVE, 1),
        (IPPROTO_TCP, TCP_KEEPIDLE, idle),
        (IPPROTO_TCP, TCP_KEEPINTVL, interval),
        (IPPROTO_TCP, TCP_KEEPCNT, count),
    ] {
        // SAFETY: `value` outlives the call and its size is passed.
        if unsafe { setsockopt(fd, level, name, &value, size_of::<c_int>() as u32) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// `CLOCK_MONOTONIC` in milliseconds: the clock of `Instant` and of
/// Chromium's `TimeTicks` (`HeadlessExperimental.beginFrame` frame times).
pub fn monotonic_ms() -> f64 {
    let mut time = Timespec { sec: 0, nsec: 0 };
    // SAFETY: valid output pointer; CLOCK_MONOTONIC always exists on Linux.
    unsafe {
        clock_gettime(CLOCK_MONOTONIC, &mut time);
    }
    time.sec as f64 * 1000.0 + time.nsec as f64 / 1e6
}

const STOP_SIGNALS: &[c_int] = &[SIGTERM, SIGINT];

fn signal_set(signals: &[c_int]) -> SigSet {
    let mut set = SigSet([0; 16]);
    // SAFETY: `set` is a valid, writable sigset_t.
    unsafe {
        sigemptyset(&mut set);
        for &signal in signals {
            sigaddset(&mut set, signal);
        }
    }
    set
}

/// Blocks `signals` in this thread and every thread started after it;
/// `wait_signal` then takes them. Children of `Command` start unblocked.
pub fn block_signals(signals: &[c_int]) {
    let set = signal_set(signals);
    // SAFETY: valid set; a null old set is allowed.
    unsafe {
        pthread_sigmask(SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Waits for one of `signals` (blocked by `block_signals`).
pub fn wait_signal(signals: &[c_int]) -> c_int {
    let set = signal_set(signals);
    let mut signal = 0;
    // SAFETY: valid set and output pointer.
    unsafe {
        sigwait(&set, &mut signal);
    }
    signal
}

/// Blocks SIGTERM and SIGINT for `wait_for_stop`. Call first thing in `main`.
pub fn block_stop_signals() {
    block_signals(STOP_SIGNALS);
}

/// Waits for SIGTERM or SIGINT (blocked by `block_stop_signals`).
pub fn wait_for_stop() -> i32 {
    wait_signal(STOP_SIGNALS)
}

/// Sends `signal` to process `pid`.
pub fn send_signal(pid: u32, signal: c_int) {
    // SAFETY: plain system call; a process already gone only fails it.
    unsafe {
        kill(pid as c_int, signal);
    }
}

/// For `CommandExt::pre_exec` in the forked child: `fds[i].0` becomes
/// descriptor `fds[i].1`, the signal mask is cleared (Chromium must not
/// inherit the blocked stop signals) and the child gets SIGKILL when the
/// sidecar dies. Only async-signal-safe calls.
pub fn child_setup(fds: &[(RawFd, RawFd)]) -> io::Result<()> {
    for &(from, to) in fds {
        // SAFETY: plain descriptor duplication in the child.
        if unsafe { dup2(from, to) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let empty = SigSet([0; 16]);
    // SAFETY: an all-zero sigset_t is the empty set on Linux; prctl takes an unsigned long.
    unsafe {
        sigprocmask(SIG_SETMASK, &empty, std::ptr::null_mut());
        if prctl(PR_SET_PDEATHSIG, SIGKILL as std::ffi::c_ulong) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// For `CommandExt::pre_exec` of the overlay's ffmpeg: descriptor `fd`
/// stays open across exec (its `pipe:N` input; none when negative), the signal mask is cleared
/// (std keeps the overlay's blocked forwarded signals, and ffmpeg would then
/// never take SIGTERM from ErsatzTV or the overlay) and ffmpeg gets SIGTERM
/// when the overlay dies. Only async-signal-safe calls.
pub fn ffmpeg_child_setup(fd: RawFd) -> io::Result<()> {
    let empty = SigSet([0; 16]);
    // SAFETY: clears FD_CLOEXEC of a descriptor the child owns; an all-zero
    // sigset_t is the empty set on Linux; prctl takes an unsigned long.
    unsafe {
        if fd >= 0 && fcntl(fd, F_SETFD, 0 as c_int) < 0 {
            return Err(io::Error::last_os_error());
        }
        sigprocmask(SIG_SETMASK, &empty, std::ptr::null_mut());
        if prctl(PR_SET_PDEATHSIG, SIGTERM as std::ffi::c_ulong) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keepalive_is_accepted_on_a_tcp_socket() {
        use std::os::fd::AsRawFd;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        keepalive(stream.as_raw_fd(), 3, 2, 4).unwrap();
        assert!(keepalive(-1, 3, 2, 4).is_err());
    }
}
