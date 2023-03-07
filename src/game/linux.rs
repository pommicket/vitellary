#![cfg(target_os = "linux")]

use crate::game::common::{GameObject, GAME_OBJECT_SIZE};
use crate::game::State;
use anyhow::{anyhow, Result};
use read_process_memory::{CopyAddress, Pid, ProcessHandle};
use std::process::Command;
use std::time::Duration;

pub(super) struct Handle {
    process: ProcessHandle,
    address: usize,
}

const DEFAULT_ADDRESS: usize = 0x854dc0;

fn get_address(pid: Pid) -> Result<usize> {
    let gdb = Command::new("gdb")
        .args(["--nw", "--nx"]) // no window, don't read .gdbinit
        .arg(format!("--pid={pid}")) // attach to pid
        .args(["--ex", "p (unsigned long long)&game"]) // print address of game object
        .args(["--ex", "set confirm off", "--ex", "q"]) // quit without confirming
        .output()?;
    if !gdb.status.success() {
        return Err(anyhow!("gdb failed with status {}", gdb.status));
    }
    let stdout = String::from_utf8_lossy(&gdb.stdout);
    // gdb should output a line like
    //     $1 = 1398429384
    // and that gives us the address of `game`.
    let pat = "\n$1 = ";
    let i = stdout
        .find(pat)
        .ok_or_else(|| anyhow!("no address in gdb output"))?;
    let stdout = &stdout[i + pat.len()..];
    let stdout = &stdout[..stdout
        .find('\n')
        .ok_or_else(|| anyhow!("weird gdb output"))?];
    Ok(stdout.parse::<usize>()?)
}

pub(super) fn find_game_object(pid: Pid) -> Result<Handle> {
    Ok(Handle {
        process: ProcessHandle::try_from(pid)?,
        address: match get_address(pid) {
            Ok(a) => a,
            Err(e) => {
                log::warn!(
                    "couldn't get address from gdb: {e}. defaulting to 0x{DEFAULT_ADDRESS:x}"
                );
                DEFAULT_ADDRESS
            }
        },
    })
}

pub(super) fn read_game_object(handle: &Handle) -> Result<(State, Duration)> {
    let mut buf = [0; GAME_OBJECT_SIZE];
    handle.process.copy_address(handle.address, &mut buf)?;
    Ok(GameObject::from(buf).into_state())
}
