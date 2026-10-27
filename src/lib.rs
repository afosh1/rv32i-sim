//! An RV32I instruction-set simulator: ELF loading, a typed decoder, a
//! one-instruction-per-step executor, instruction traces and VCD waveform output.
//!
//! ```no_run
//! let bytes = std::fs::read("rv32ui-p-add").unwrap();
//! let mut cpu = rv32i_sim::load_program(&bytes, 0x8000_0000, 1 << 20).unwrap();
//! let exit = cpu.run(100_000, |_, _| {}).unwrap();
//! assert_eq!(exit.code, 0);
//! ```

pub mod cpu;
pub mod decode;
pub mod elf;
pub mod memory;
pub mod trace;

use std::fmt;

pub use cpu::{Commit, Cpu, Exit, SimError};
use elf::ElfError;
use memory::{AccessFault, Memory};

#[derive(Debug)]
pub enum LoadError {
    Elf(ElfError),
    DoesNotFit(AccessFault),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Elf(e) => write!(f, "{e}"),
            LoadError::DoesNotFit(fault) => write!(f, "program does not fit in RAM: {fault}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<ElfError> for LoadError {
    fn from(e: ElfError) -> Self {
        LoadError::Elf(e)
    }
}

impl From<AccessFault> for LoadError {
    fn from(fault: AccessFault) -> Self {
        LoadError::DoesNotFit(fault)
    }
}

/// Build a CPU with `bytes` loaded: an ELF32 is placed by its program headers,
/// anything else is treated as a raw image at `mem_base` and entered there.
pub fn load_program(bytes: &[u8], mem_base: u32, mem_size: usize) -> Result<Cpu, LoadError> {
    let mut mem = Memory::new(mem_base, mem_size);
    let entry = if elf::is_elf(bytes) {
        let image = elf::parse(bytes)?;
        for segment in &image.segments {
            mem.load(segment.addr, &segment.data)?;
        }
        image.entry
    } else {
        mem.load(mem_base, bytes)?;
        mem_base
    };
    Ok(Cpu::new(mem, entry))
}
