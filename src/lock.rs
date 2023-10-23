use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::process;
use sysinfo::{PidExt, System, SystemExt};

/* This locking mechanism is meant to never be deleted. Instead, it stores the PID of the process
 * that's running, when trying to aquire a lock, it checks wether that process is still running. If
 * not, it rewrites the lockfile to have its own PID instead. */

pub static LOCKFILE: &str = "rewatch.lock";

pub enum Error {
    Locked(u32),
    ParsingLockfile(std::num::ParseIntError),
    ReadingLockfile(std::io::Error),
    WritingLockfile(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let msg = match self {
            Error::Locked(pid) => format!("Rewatch is already running with PID {}", pid),
            Error::ParsingLockfile(e) => format!("Could not parse lockfile: \n {}", e.to_string()),
            Error::ReadingLockfile(e) => format!("Could not read lockfile: \n {}", e.to_string()),
            Error::WritingLockfile(e) => format!("Could not write lockfile: \n {}", e.to_string()),
        };
        write!(f, "{}", msg)
    }
}

pub enum Lock {
    Locked(u32),
    Error(Error),
}

fn exists(to_check_pid: u32) -> bool {
    System::new_all()
        .processes()
        .into_iter()
        .any(|(pid, _process)| pid.as_u32() == to_check_pid)
}

fn create(pid: u32) -> Lock {
    File::create(LOCKFILE)
        .and_then(|mut file| file.write(pid.to_string().as_bytes()).map(|_| Lock::Locked(pid)))
        .unwrap_or_else(|e| Lock::Error(Error::WritingLockfile(e)))
}

pub fn get() -> Lock {
    let pid = process::id();

    match fs::read_to_string(LOCKFILE) {
        Err(e) if (e.kind() == std::io::ErrorKind::NotFound) => create(pid),
        Err(e) => Lock::Error(Error::ReadingLockfile(e)),
        Ok(s) => match s.parse::<u32>() {
            Ok(parsed_pid) if !exists(parsed_pid) => create(pid),
            Ok(parsed_pid) => Lock::Error(Error::Locked(parsed_pid)),
            Err(e) => Lock::Error(Error::ParsingLockfile(e)),
        },
    }
}
