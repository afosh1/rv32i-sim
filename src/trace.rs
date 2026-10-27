//! Views of a run: a one-line-per-instruction trace and a VCD waveform.

use std::fmt::Write as _;
use std::io::{self, Write};

use crate::cpu::{Commit, Cpu};

/// `   instret  pc        raw       disassembly                   effects`
pub fn format_commit(instret: u64, c: &Commit) -> String {
    let mut line = format!(
        "{instret:>8}  {:08x}  {:08x}  {:<28}",
        c.pc,
        c.raw,
        c.inst.to_string()
    );
    if let Some((rd, value)) = c.rd_write {
        let _ = write!(line, "  x{rd:<2} <- {value:#010x}");
    }
    if let Some(w) = c.mem_write {
        let digits = 2 * w.size as usize;
        let _ = write!(
            line,
            "  mem[{:#010x}] <- {:#0width$x}",
            w.addr,
            w.value,
            width = digits + 2
        );
    }
    line.trim_end().to_string()
}

// VCD identifier codes are short printable-ASCII strings; one char each is plenty.
const ID_CLK: char = '!';
const ID_PC: char = '"';
const ID_INSTR: char = '#';

fn reg_id(r: u8) -> char {
    char::from(b'$' + r)
}

/// Clock period in VCD time units: each retired instruction is one cycle.
const PERIOD: u64 = 10;

/// Streams a Value Change Dump with `clk`, `pc`, `instr` and `x1`-`x31`, so a
/// software run can be inspected in GTKWave/Surfer next to an RTL simulation.
///
/// Write errors are sticky (like C's `ferror`): `record` never fails, and the
/// first error is reported by `finish`.
pub struct VcdWriter<W: Write> {
    out: W,
    regs: [u32; 32],
    time: u64,
    error: Option<io::Error>,
}

impl<W: Write> VcdWriter<W> {
    pub fn new(mut out: W, cpu: &Cpu) -> io::Result<Self> {
        writeln!(out, "$version rv32i-sim {} $end", env!("CARGO_PKG_VERSION"))?;
        writeln!(out, "$timescale 1ns $end")?;
        writeln!(out, "$scope module hart0 $end")?;
        writeln!(out, "$var wire 1 {ID_CLK} clk $end")?;
        writeln!(out, "$var wire 32 {ID_PC} pc $end")?;
        writeln!(out, "$var wire 32 {ID_INSTR} instr $end")?;
        for r in 1..32u8 {
            writeln!(out, "$var wire 32 {} x{r} $end", reg_id(r))?;
        }
        writeln!(out, "$upscope $end")?;
        writeln!(out, "$enddefinitions $end")?;
        writeln!(out, "#0")?;
        writeln!(out, "$dumpvars")?;
        writeln!(out, "0{ID_CLK}")?;
        writeln!(out, "b{:b} {ID_PC}", cpu.pc())?;
        writeln!(out, "bx {ID_INSTR}")?;
        for r in 1..32u8 {
            writeln!(out, "b{:b} {}", cpu.reg(r), reg_id(r))?;
        }
        writeln!(out, "$end")?;
        Ok(Self {
            out,
            regs: *cpu.regs(),
            time: 0,
            error: None,
        })
    }

    /// Rising edge: the instruction's pc/instr appear. Falling edge: its result is written back.
    pub fn record(&mut self, c: &Commit) {
        if self.error.is_none()
            && let Err(e) = self.try_record(c)
        {
            self.error = Some(e);
        }
    }

    fn try_record(&mut self, c: &Commit) -> io::Result<()> {
        self.time += PERIOD;
        writeln!(self.out, "#{}", self.time)?;
        writeln!(self.out, "1{ID_CLK}")?;
        writeln!(self.out, "b{:b} {ID_PC}", c.pc)?;
        writeln!(self.out, "b{:b} {ID_INSTR}", c.raw)?;
        writeln!(self.out, "#{}", self.time + PERIOD / 2)?;
        writeln!(self.out, "0{ID_CLK}")?;
        if let Some((rd, value)) = c.rd_write
            && self.regs[usize::from(rd)] != value
        {
            self.regs[usize::from(rd)] = value;
            writeln!(self.out, "b{value:b} {}", reg_id(rd))?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<W> {
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        self.out.flush()?;
        Ok(self.out)
    }
}
