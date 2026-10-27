//! WebAssembly exports for the in-browser demo.
//!
//! A plain C ABI instead of wasm-bindgen: JavaScript calls these functions on
//! the instance and reads text straight out of linear memory. Each
//! `WebAssembly.instantiate` has its own memory, so one instance = one simulator.
//!
//! Build: `cargo build -p rv32i-sim-web --release --target wasm32-unknown-unknown`

use std::cell::RefCell;

use rv32i_sim::{Cpu, SimError, load_program};

/// Programs the page can load, by index.
const PROGRAMS: [&[u8]; 3] = [
    include_bytes!("../programs/collatz.elf"),
    include_bytes!("../programs/fibonacci.bin"),
    include_bytes!("../programs/rv32ui-p-add"),
];

const MEM_BASE: u32 = 0x8000_0000;
const MEM_SIZE: usize = 64 * 1024;
const REG_A0: u8 = 10;
const REG_A7: u8 = 17;
const SYS_EXIT: u32 = 93;

pub const RUNNING: i32 = 0;
pub const EXITED: i32 = 1;
pub const FAULTED: i32 = 2;

struct Sim {
    cpu: Cpu,
    status: i32,
    last_pc: u32,
    last_raw: u32,
    last_rd: i32,
    /// Disassembly of the last instruction, or the exit/fault message.
    text: String,
}

thread_local! {
    static SIM: RefCell<Option<Sim>> = const { RefCell::new(None) };
}

fn with_sim<T>(default: T, f: impl FnOnce(&mut Sim) -> T) -> T {
    SIM.with_borrow_mut(|sim| sim.as_mut().map_or(default, f))
}

/// Load program `index`; returns 0 on success, -1 for an unknown index or a bad image.
#[unsafe(no_mangle)]
pub extern "C" fn sim_load(index: u32) -> i32 {
    let Some(bytes) = PROGRAMS.get(index as usize) else {
        return -1;
    };
    let Ok(cpu) = load_program(bytes, MEM_BASE, MEM_SIZE) else {
        return -1;
    };
    let sim = Sim {
        last_pc: cpu.pc(),
        cpu,
        status: RUNNING,
        last_raw: 0,
        last_rd: -1,
        text: String::new(),
    };
    SIM.set(Some(sim));
    0
}

/// Execute one instruction and return the new status (RUNNING, EXITED or FAULTED).
#[unsafe(no_mangle)]
pub extern "C" fn sim_step() -> i32 {
    with_sim(FAULTED, |sim| {
        if sim.status != RUNNING {
            return sim.status;
        }
        match sim.cpu.step() {
            Ok(commit) => {
                sim.last_pc = commit.pc;
                sim.last_raw = commit.raw;
                sim.last_rd = commit.rd_write.map_or(-1, |(rd, _)| i32::from(rd));
                sim.text = commit.inst.to_string();
                if commit.raw == 0x0000_0073 {
                    let number = sim.cpu.reg(REG_A7);
                    sim.status = if number == SYS_EXIT { EXITED } else { FAULTED };
                    sim.text = if number == SYS_EXIT {
                        format!("exit({})", sim.cpu.reg(REG_A0))
                    } else {
                        SimError::UnsupportedEcall {
                            pc: commit.pc,
                            number,
                        }
                        .to_string()
                    };
                }
            }
            Err(e) => {
                sim.status = FAULTED;
                sim.text = e.to_string();
            }
        }
        sim.status
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sim_pc() -> u32 {
    with_sim(0, |sim| sim.cpu.pc())
}

#[unsafe(no_mangle)]
pub extern "C" fn sim_reg(r: u32) -> u32 {
    with_sim(0, |sim| {
        sim.cpu.regs().get(r as usize).copied().unwrap_or(0)
    })
}

/// Instructions retired so far (saturating: the demo never gets near 2^32).
#[unsafe(no_mangle)]
pub extern "C" fn sim_instret() -> u32 {
    with_sim(0, |sim| {
        u32::try_from(sim.cpu.instret()).unwrap_or(u32::MAX)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sim_last_pc() -> u32 {
    with_sim(0, |sim| sim.last_pc)
}

#[unsafe(no_mangle)]
pub extern "C" fn sim_last_raw() -> u32 {
    with_sim(0, |sim| sim.last_raw)
}

/// Register written by the last instruction, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn sim_last_rd() -> i32 {
    with_sim(-1, |sim| sim.last_rd)
}

#[unsafe(no_mangle)]
pub extern "C" fn sim_text_ptr() -> *const u8 {
    with_sim(std::ptr::null(), |sim| sim.text.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn sim_text_len() -> usize {
    with_sim(0, |sim| sim.text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_to_end(index: u32, limit: u32) -> (i32, String) {
        assert_eq!(sim_load(index), 0);
        let mut status = RUNNING;
        for _ in 0..limit {
            status = sim_step();
            if status != RUNNING {
                break;
            }
        }
        (
            status,
            SIM.with_borrow(|s| s.as_ref().unwrap().text.clone()),
        )
    }

    #[test]
    fn collatz_exits_with_111() {
        assert_eq!(run_to_end(0, 100_000), (EXITED, "exit(111)".to_string()));
    }

    #[test]
    fn fibonacci_runs_forever() {
        let (status, _) = run_to_end(1, 1_000);
        assert_eq!(status, RUNNING);
    }

    #[test]
    fn riscv_test_passes() {
        assert_eq!(run_to_end(2, 100_000), (EXITED, "exit(0)".to_string()));
    }
}
