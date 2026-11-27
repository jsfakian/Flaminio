//! Small safe Linux syscall boundary for the daemon's qualified clock backend.
#![allow(unsafe_code)]

use std::io;
use std::os::fd::AsRawFd;

#[derive(Clone, Copy)]
pub enum ElapsedSource {
    Boottime,
    Monotonic,
}

fn clock_id(source: ElapsedSource) -> libc::clockid_t {
    match source {
        ElapsedSource::Boottime => libc::CLOCK_BOOTTIME,
        ElapsedSource::Monotonic => libc::CLOCK_MONOTONIC,
    }
}

pub fn elapsed_time(source: ElapsedSource) -> io::Result<(i64, i64)> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts points to valid writable storage, and clock_id returns a valid Linux ID.
    if unsafe { libc::clock_gettime(clock_id(source), &mut ts) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((ts.tv_sec, ts.tv_nsec))
}

pub fn elapsed_resolution(source: ElapsedSource) -> io::Result<(i64, i64)> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts points to valid writable storage, and clock_id returns a valid Linux ID.
    if unsafe { libc::clock_getres(clock_id(source), &mut ts) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((ts.tv_sec, ts.tv_nsec))
}

/// Inspect the mount of the actual opened input, including nested provenance mounts.
pub fn is_readonly(file: &impl AsRawFd) -> io::Result<bool> {
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: file's borrowed descriptor is live; stat is valid writable output storage.
    if unsafe { libc::fstatvfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatvfs initialized stat.
    Ok(unsafe { stat.assume_init() }.f_flag & libc::ST_RDONLY != 0)
}

/// Linux getrandom(flags=0) waits for initialized kernel entropy. A short read or
/// syscall error refuses startup; there is no file, timer or PRNG fallback.
pub fn process_nonce() -> io::Result<[u8; 32]> {
    let mut nonce = [0u8; 32];
    // SAFETY: nonce is writable for exactly the passed length, with no concurrent access.
    let count = unsafe { libc::getrandom(nonce.as_mut_ptr().cast(), nonce.len(), 0) };
    if count < 0 {
        return Err(io::Error::last_os_error());
    }
    if count != nonce.len() as isize || nonce.iter().all(|&b| b == 0) {
        return Err(io::Error::other("invalid OS process challenge"));
    }
    Ok(nonce)
}
