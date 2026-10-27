//! Architectural state of one RV32I hart and the fetch -> decode -> execute -> write-back step.

use std::fmt;

use crate::decode::{AluOp, BranchKind, CsrOp, Instruction, LoadWidth, StoreWidth, decode};
use crate::memory::{AccessFault, Memory};

const CSR_MEPC: u16 = 0x341;
const CSR_CYCLE: u16 = 0xc00;
const CSR_TIME: u16 = 0xc01;
const CSR_INSTRET: u16 = 0xc02;
const CSR_CYCLEH: u16 = 0xc80;
const CSR_TIMEH: u16 = 0xc81;
const CSR_INSTRETH: u16 = 0xc82;

/// `a7` value for the Linux/newlib `exit` syscall. riscv-tests report pass/fail through it.
pub const SYS_EXIT: u32 = 93;
const REG_A0: u8 = 10;
const REG_A7: u8 = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemWrite {
    pub addr: u32,
    pub size: u32,
    pub value: u32,
}

/// Everything one retired instruction changed: the input to traces and waveforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Commit {
    pub pc: u32,
    pub raw: u32,
    pub inst: Instruction,
    /// Destination register and its new value. `None` for x0, which ignores writes.
    pub rd_write: Option<(u8, u32)>,
    pub mem_write: Option<MemWrite>,
}

/// How a run ended successfully: the program called `exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    pub code: u32,
    pub steps: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    FetchFault { pc: u32, fault: AccessFault },
    IllegalInstruction { pc: u32, raw: u32 },
    LoadFault { pc: u32, fault: AccessFault },
    StoreFault { pc: u32, fault: AccessFault },
    MisalignedJump { pc: u32, target: u32 },
    Breakpoint { pc: u32 },
    UnsupportedEcall { pc: u32, number: u32 },
    StepLimit { steps: u64 },
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::FetchFault { pc, fault } => write!(f, "pc {pc:#010x}: fetch failed: {fault}"),
            SimError::IllegalInstruction { pc, raw } => {
                write!(f, "pc {pc:#010x}: illegal instruction {raw:#010x}")
            }
            SimError::LoadFault { pc, fault } => write!(f, "pc {pc:#010x}: load failed: {fault}"),
            SimError::StoreFault { pc, fault } => write!(f, "pc {pc:#010x}: store failed: {fault}"),
            SimError::MisalignedJump { pc, target } => {
                write!(
                    f,
                    "pc {pc:#010x}: jump to misaligned address {target:#010x}"
                )
            }
            SimError::Breakpoint { pc } => write!(f, "pc {pc:#010x}: ebreak"),
            SimError::UnsupportedEcall { pc, number } => {
                write!(f, "pc {pc:#010x}: unsupported ecall (a7 = {number})")
            }
            SimError::StepLimit { steps } => write!(f, "no exit after {steps} instructions"),
        }
    }
}

impl std::error::Error for SimError {}

pub struct Cpu {
    pc: u32,
    regs: [u32; 32],
    /// Flat CSR file. Enough for riscv-tests' machine-mode setup code, which only
    /// needs writes to stick and `mepc` to drive `mret`; no trap delivery yet.
    csrs: Box<[u32; 4096]>,
    instret: u64,
    pub mem: Memory,
}

impl Cpu {
    /// A hart that starts at `entry` with `sp` at the top of RAM and everything else zero.
    pub fn new(mem: Memory, entry: u32) -> Self {
        let mut regs = [0; 32];
        regs[2] = u32::try_from(mem.end()).unwrap_or(u32::MAX) & !0xf;
        Self {
            pc: entry,
            regs,
            csrs: Box::new([0; 4096]),
            instret: 0,
            mem,
        }
    }

    pub fn pc(&self) -> u32 {
        self.pc
    }

    pub fn reg(&self, r: u8) -> u32 {
        self.regs[usize::from(r)]
    }

    pub fn regs(&self) -> &[u32; 32] {
        &self.regs
    }

    /// Instructions retired so far.
    pub fn instret(&self) -> u64 {
        self.instret
    }

    fn csr_read(&self, csr: u16) -> u32 {
        match csr {
            // One instruction per cycle, so all three counters agree.
            CSR_CYCLE | CSR_TIME | CSR_INSTRET => self.instret as u32,
            CSR_CYCLEH | CSR_TIMEH | CSR_INSTRETH => (self.instret >> 32) as u32,
            _ => self.csrs[usize::from(csr)],
        }
    }

    /// Zicsr read-modify-write; returns the old value for `rd`.
    /// CSRRS/CSRRC with a zero source must not write at all, per the spec.
    fn csr_access(&mut self, op: CsrOp, csr: u16, src: u32, src_is_zero: bool) -> u32 {
        let old = self.csr_read(csr);
        let new = match op {
            CsrOp::Write => Some(src),
            CsrOp::Set => (!src_is_zero).then_some(old | src),
            CsrOp::Clear => (!src_is_zero).then_some(old & !src),
        };
        if let Some(value) = new {
            self.csrs[usize::from(csr)] = value;
        }
        old
    }

    fn load(&self, width: LoadWidth, addr: u32) -> Result<u32, AccessFault> {
        // Sign extension is a cast chain: narrow to the signed width, widen to i32.
        Ok(match width {
            LoadWidth::Byte => self.mem.read(addr, 1)? as u8 as i8 as i32 as u32,
            LoadWidth::Half => self.mem.read(addr, 2)? as u16 as i16 as i32 as u32,
            LoadWidth::Word => self.mem.read(addr, 4)?,
            LoadWidth::ByteU => self.mem.read(addr, 1)?,
            LoadWidth::HalfU => self.mem.read(addr, 2)?,
        })
    }

    /// Execute exactly one instruction.
    pub fn step(&mut self) -> Result<Commit, SimError> {
        let pc = self.pc;
        let raw = self
            .mem
            .read(pc, 4)
            .map_err(|fault| SimError::FetchFault { pc, fault })?;
        let inst = decode(raw).map_err(|_| SimError::IllegalInstruction { pc, raw })?;

        let mut next_pc = pc.wrapping_add(4);
        let mut rd_write = None;
        let mut mem_write = None;

        match inst {
            Instruction::Lui { rd, imm } => rd_write = Some((rd, imm)),
            Instruction::Auipc { rd, imm } => rd_write = Some((rd, pc.wrapping_add(imm))),
            Instruction::Jal { rd, offset } => {
                rd_write = Some((rd, pc.wrapping_add(4)));
                next_pc = pc.wrapping_add_signed(offset);
            }
            Instruction::Jalr { rd, rs1, offset } => {
                rd_write = Some((rd, pc.wrapping_add(4)));
                next_pc = self.reg(rs1).wrapping_add_signed(offset) & !1;
            }
            Instruction::Branch {
                kind,
                rs1,
                rs2,
                offset,
            } => {
                if branch_taken(kind, self.reg(rs1), self.reg(rs2)) {
                    next_pc = pc.wrapping_add_signed(offset);
                }
            }
            Instruction::Load {
                width,
                rd,
                rs1,
                offset,
            } => {
                let addr = self.reg(rs1).wrapping_add_signed(offset);
                let value = self
                    .load(width, addr)
                    .map_err(|fault| SimError::LoadFault { pc, fault })?;
                rd_write = Some((rd, value));
            }
            Instruction::Store {
                width,
                rs1,
                rs2,
                offset,
            } => {
                let addr = self.reg(rs1).wrapping_add_signed(offset);
                let size = match width {
                    StoreWidth::Byte => 1,
                    StoreWidth::Half => 2,
                    StoreWidth::Word => 4,
                };
                let value = self.reg(rs2) & (u32::MAX >> (32 - 8 * size));
                self.mem
                    .write(addr, size, value)
                    .map_err(|fault| SimError::StoreFault { pc, fault })?;
                mem_write = Some(MemWrite { addr, size, value });
            }
            Instruction::OpImm { op, rd, rs1, imm } => {
                rd_write = Some((rd, alu(op, self.reg(rs1), imm as u32)));
            }
            Instruction::Op { op, rd, rs1, rs2 } => {
                rd_write = Some((rd, alu(op, self.reg(rs1), self.reg(rs2))));
            }
            // An ecall retires normally; `run` decides what the host does with it.
            Instruction::Fence | Instruction::Ecall => {}
            Instruction::Ebreak => return Err(SimError::Breakpoint { pc }),
            Instruction::Mret => next_pc = self.csr_read(CSR_MEPC),
            Instruction::Csr { op, rd, rs1, csr } => {
                let old = self.csr_access(op, csr, self.reg(rs1), rs1 == 0);
                rd_write = Some((rd, old));
            }
            Instruction::CsrImm { op, rd, zimm, csr } => {
                let old = self.csr_access(op, csr, u32::from(zimm), zimm == 0);
                rd_write = Some((rd, old));
            }
        }

        // Without the C extension every instruction address is 4-byte aligned.
        if !next_pc.is_multiple_of(4) {
            return Err(SimError::MisalignedJump {
                pc,
                target: next_pc,
            });
        }

        let rd_write = rd_write.filter(|&(rd, _)| rd != 0);
        if let Some((rd, value)) = rd_write {
            self.regs[usize::from(rd)] = value;
        }
        self.pc = next_pc;
        self.instret += 1;
        Ok(Commit {
            pc,
            raw,
            inst,
            rd_write,
            mem_write,
        })
    }

    /// Run until the program calls `exit` (`ecall` with a7 = 93), reporting every
    /// retired instruction to `on_commit`.
    pub fn run(
        &mut self,
        max_steps: u64,
        mut on_commit: impl FnMut(&Cpu, &Commit),
    ) -> Result<Exit, SimError> {
        while self.instret < max_steps {
            let commit = self.step()?;
            on_commit(self, &commit);
            if commit.inst == Instruction::Ecall {
                let number = self.reg(REG_A7);
                if number != SYS_EXIT {
                    return Err(SimError::UnsupportedEcall {
                        pc: commit.pc,
                        number,
                    });
                }
                return Ok(Exit {
                    code: self.reg(REG_A0),
                    steps: self.instret,
                });
            }
        }
        Err(SimError::StepLimit { steps: max_steps })
    }
}

fn branch_taken(kind: BranchKind, a: u32, b: u32) -> bool {
    match kind {
        BranchKind::Eq => a == b,
        BranchKind::Ne => a != b,
        BranchKind::Lt => (a as i32) < (b as i32),
        BranchKind::Ge => (a as i32) >= (b as i32),
        BranchKind::Ltu => a < b,
        BranchKind::Geu => a >= b,
    }
}

/// The integer ALU shared by OP (register-register) and OP-IMM instructions.
///
/// Registers hold raw `u32` bit patterns. Signedness belongs to the *operation*,
/// not the value, exactly as in hardware: SLT compares as signed, SLTU as unsigned,
/// SRA shifts in copies of the sign bit, SRL shifts in zeros.
pub fn alu(op: AluOp, a: u32, b: u32) -> u32 {
    // Shifts use only the low 5 bits of the amount (RV32I).
    let shamt = b & 0x1f;
    match op {
        AluOp::Add => a.wrapping_add(b),
        AluOp::Sub => a.wrapping_sub(b),
        AluOp::Sll => a << shamt,
        AluOp::Slt => u32::from((a as i32) < (b as i32)),
        AluOp::Sltu => u32::from(a < b),
        AluOp::Xor => a ^ b,
        AluOp::Srl => a >> shamt,
        AluOp::Sra => ((a as i32) >> shamt) as u32,
        AluOp::Or => a | b,
        AluOp::And => a & b,
    }
}
