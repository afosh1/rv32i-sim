//! Instruction decoding: raw 32-bit words -> typed [`Instruction`]s.
//!
//! Decoding happens once, up front, into an enum whose variants only carry the
//! fields that instruction actually has. The executor then pattern-matches on
//! meaning ("this is a BLT") instead of re-inspecting opcode/funct3/funct7 bits.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluOp {
    Add,
    Sub,
    Sll,
    Slt,
    Sltu,
    Xor,
    Srl,
    Sra,
    Or,
    And,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    Eq,
    Ne,
    Lt,
    Ge,
    Ltu,
    Geu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadWidth {
    Byte,
    Half,
    Word,
    ByteU,
    HalfU,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreWidth {
    Byte,
    Half,
    Word,
}

/// The read-modify-write flavour of a Zicsr instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsrOp {
    /// CSRRW: write the source value.
    Write,
    /// CSRRS: set the bits that are 1 in the source.
    Set,
    /// CSRRC: clear the bits that are 1 in the source.
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    Lui {
        rd: u8,
        imm: u32,
    },
    Auipc {
        rd: u8,
        imm: u32,
    },
    Jal {
        rd: u8,
        offset: i32,
    },
    Jalr {
        rd: u8,
        rs1: u8,
        offset: i32,
    },
    Branch {
        kind: BranchKind,
        rs1: u8,
        rs2: u8,
        offset: i32,
    },
    Load {
        width: LoadWidth,
        rd: u8,
        rs1: u8,
        offset: i32,
    },
    Store {
        width: StoreWidth,
        rs1: u8,
        rs2: u8,
        offset: i32,
    },
    OpImm {
        op: AluOp,
        rd: u8,
        rs1: u8,
        imm: i32,
    },
    Op {
        op: AluOp,
        rd: u8,
        rs1: u8,
        rs2: u8,
    },
    /// FENCE and FENCE.I. Both are no-ops for a single in-order hart that
    /// re-fetches every instruction from memory (so self-modifying code works).
    Fence,
    Ecall,
    Ebreak,
    Mret,
    /// CSR access with a register source (CSRRW / CSRRS / CSRRC).
    Csr {
        op: CsrOp,
        rd: u8,
        rs1: u8,
        csr: u16,
    },
    /// CSR access with a 5-bit zero-extended immediate (CSRRWI / CSRRSI / CSRRCI).
    CsrImm {
        op: CsrOp,
        rd: u8,
        zimm: u8,
        csr: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalInstruction(pub u32);

impl fmt::Display for IllegalInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "illegal instruction {:#010x}", self.0)
    }
}

impl std::error::Error for IllegalInstruction {}

// --- Immediate reconstruction -------------------------------------------------
//
// Every format keeps the sign bit at instruction bit 31, so each helper starts
// by casting to i32 and arithmetic-shifting: the shift does the sign extension
// that the C++ version did by hand with `imm |= 0xFFFFE000`.

fn imm_i(raw: u32) -> i32 {
    (raw as i32) >> 20
}

fn imm_s(raw: u32) -> i32 {
    (((raw & 0xfe00_0000) as i32) >> 20) // imm[11:5] <- inst[31:25]
        | ((raw >> 7) & 0x1f) as i32 // imm[4:0]  <- inst[11:7]
}

fn imm_b(raw: u32) -> i32 {
    (((raw & 0x8000_0000) as i32) >> 19) // imm[12]   <- inst[31]
        | ((raw & 0x80) << 4) as i32 // imm[11]   <- inst[7]
        | ((raw >> 20) & 0x7e0) as i32 // imm[10:5] <- inst[30:25]
        | ((raw >> 7) & 0x1e) as i32 // imm[4:1]  <- inst[11:8]
}

fn imm_u(raw: u32) -> u32 {
    raw & 0xffff_f000
}

fn imm_j(raw: u32) -> i32 {
    (((raw & 0x8000_0000) as i32) >> 11) // imm[20]    <- inst[31]
        | (raw & 0x000f_f000) as i32 // imm[19:12] <- inst[19:12]
        | ((raw >> 9) & 0x800) as i32 // imm[11]    <- inst[20]
        | ((raw >> 20) & 0x7fe) as i32 // imm[10:1]  <- inst[30:21]
}

pub fn decode(raw: u32) -> Result<Instruction, IllegalInstruction> {
    use Instruction::*;

    let illegal = Err(IllegalInstruction(raw));
    let opcode = raw & 0x7f;
    let rd = ((raw >> 7) & 0x1f) as u8;
    let funct3 = (raw >> 12) & 0x7;
    let rs1 = ((raw >> 15) & 0x1f) as u8;
    let rs2 = ((raw >> 20) & 0x1f) as u8;
    let funct7 = raw >> 25;

    let inst = match opcode {
        0x37 => Lui {
            rd,
            imm: imm_u(raw),
        },
        0x17 => Auipc {
            rd,
            imm: imm_u(raw),
        },
        0x6f => Jal {
            rd,
            offset: imm_j(raw),
        },
        0x67 if funct3 == 0 => Jalr {
            rd,
            rs1,
            offset: imm_i(raw),
        },
        0x63 => {
            let kind = match funct3 {
                0 => BranchKind::Eq,
                1 => BranchKind::Ne,
                4 => BranchKind::Lt,
                5 => BranchKind::Ge,
                6 => BranchKind::Ltu,
                7 => BranchKind::Geu,
                _ => return illegal,
            };
            Branch {
                kind,
                rs1,
                rs2,
                offset: imm_b(raw),
            }
        }
        0x03 => {
            let width = match funct3 {
                0 => LoadWidth::Byte,
                1 => LoadWidth::Half,
                2 => LoadWidth::Word,
                4 => LoadWidth::ByteU,
                5 => LoadWidth::HalfU,
                _ => return illegal,
            };
            Load {
                width,
                rd,
                rs1,
                offset: imm_i(raw),
            }
        }
        0x23 => {
            let width = match funct3 {
                0 => StoreWidth::Byte,
                1 => StoreWidth::Half,
                2 => StoreWidth::Word,
                _ => return illegal,
            };
            Store {
                width,
                rs1,
                rs2,
                offset: imm_s(raw),
            }
        }
        0x13 => {
            let op = match (funct3, funct7) {
                (0, _) => AluOp::Add,
                (2, _) => AluOp::Slt,
                (3, _) => AluOp::Sltu,
                (4, _) => AluOp::Xor,
                (6, _) => AluOp::Or,
                (7, _) => AluOp::And,
                // Shift-immediates reuse funct7 to pick logical vs arithmetic.
                (1, 0x00) => AluOp::Sll,
                (5, 0x00) => AluOp::Srl,
                (5, 0x20) => AluOp::Sra,
                _ => return illegal,
            };
            // For shifts the immediate is only the 5-bit shamt in the rs2 slot.
            let imm = match op {
                AluOp::Sll | AluOp::Srl | AluOp::Sra => i32::from(rs2),
                _ => imm_i(raw),
            };
            OpImm { op, rd, rs1, imm }
        }
        0x33 => {
            let op = match (funct3, funct7) {
                (0, 0x00) => AluOp::Add,
                (0, 0x20) => AluOp::Sub,
                (1, 0x00) => AluOp::Sll,
                (2, 0x00) => AluOp::Slt,
                (3, 0x00) => AluOp::Sltu,
                (4, 0x00) => AluOp::Xor,
                (5, 0x00) => AluOp::Srl,
                (5, 0x20) => AluOp::Sra,
                (6, 0x00) => AluOp::Or,
                (7, 0x00) => AluOp::And,
                _ => return illegal,
            };
            Op { op, rd, rs1, rs2 }
        }
        0x0f if funct3 <= 1 => Fence,
        0x73 => {
            let csr = (raw >> 20) as u16;
            match funct3 {
                0 => match raw {
                    0x0000_0073 => Ecall,
                    0x0010_0073 => Ebreak,
                    0x3020_0073 => Mret,
                    _ => return illegal,
                },
                1 => Csr {
                    op: CsrOp::Write,
                    rd,
                    rs1,
                    csr,
                },
                2 => Csr {
                    op: CsrOp::Set,
                    rd,
                    rs1,
                    csr,
                },
                3 => Csr {
                    op: CsrOp::Clear,
                    rd,
                    rs1,
                    csr,
                },
                5 => CsrImm {
                    op: CsrOp::Write,
                    rd,
                    zimm: rs1,
                    csr,
                },
                6 => CsrImm {
                    op: CsrOp::Set,
                    rd,
                    zimm: rs1,
                    csr,
                },
                7 => CsrImm {
                    op: CsrOp::Clear,
                    rd,
                    zimm: rs1,
                    csr,
                },
                _ => return illegal,
            }
        }
        _ => return illegal,
    };
    Ok(inst)
}

// --- Disassembly ----------------------------------------------------------------

impl AluOp {
    fn mnemonic(self) -> &'static str {
        match self {
            AluOp::Add => "add",
            AluOp::Sub => "sub",
            AluOp::Sll => "sll",
            AluOp::Slt => "slt",
            AluOp::Sltu => "sltu",
            AluOp::Xor => "xor",
            AluOp::Srl => "srl",
            AluOp::Sra => "sra",
            AluOp::Or => "or",
            AluOp::And => "and",
        }
    }

    fn imm_mnemonic(self) -> &'static str {
        match self {
            AluOp::Add => "addi",
            AluOp::Sll => "slli",
            AluOp::Slt => "slti",
            AluOp::Sltu => "sltiu",
            AluOp::Xor => "xori",
            AluOp::Srl => "srli",
            AluOp::Sra => "srai",
            AluOp::Or => "ori",
            AluOp::And => "andi",
            AluOp::Sub => unreachable!("RV32I has no subi"),
        }
    }
}

impl BranchKind {
    fn mnemonic(self) -> &'static str {
        match self {
            BranchKind::Eq => "beq",
            BranchKind::Ne => "bne",
            BranchKind::Lt => "blt",
            BranchKind::Ge => "bge",
            BranchKind::Ltu => "bltu",
            BranchKind::Geu => "bgeu",
        }
    }
}

impl LoadWidth {
    fn mnemonic(self) -> &'static str {
        match self {
            LoadWidth::Byte => "lb",
            LoadWidth::Half => "lh",
            LoadWidth::Word => "lw",
            LoadWidth::ByteU => "lbu",
            LoadWidth::HalfU => "lhu",
        }
    }
}

impl StoreWidth {
    fn mnemonic(self) -> &'static str {
        match self {
            StoreWidth::Byte => "sb",
            StoreWidth::Half => "sh",
            StoreWidth::Word => "sw",
        }
    }
}

impl CsrOp {
    fn mnemonic(self, imm: bool) -> &'static str {
        match (self, imm) {
            (CsrOp::Write, false) => "csrrw",
            (CsrOp::Set, false) => "csrrs",
            (CsrOp::Clear, false) => "csrrc",
            (CsrOp::Write, true) => "csrrwi",
            (CsrOp::Set, true) => "csrrsi",
            (CsrOp::Clear, true) => "csrrci",
        }
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Instruction::*;
        match *self {
            Lui { rd, imm } => write!(f, "lui x{rd}, {:#x}", imm >> 12),
            Auipc { rd, imm } => write!(f, "auipc x{rd}, {:#x}", imm >> 12),
            Jal { rd, offset } => write!(f, "jal x{rd}, {offset}"),
            Jalr { rd, rs1, offset } => write!(f, "jalr x{rd}, {offset}(x{rs1})"),
            Branch {
                kind,
                rs1,
                rs2,
                offset,
            } => {
                write!(f, "{} x{rs1}, x{rs2}, {offset}", kind.mnemonic())
            }
            Load {
                width,
                rd,
                rs1,
                offset,
            } => {
                write!(f, "{} x{rd}, {offset}(x{rs1})", width.mnemonic())
            }
            Store {
                width,
                rs1,
                rs2,
                offset,
            } => {
                write!(f, "{} x{rs2}, {offset}(x{rs1})", width.mnemonic())
            }
            OpImm { op, rd, rs1, imm } => write!(f, "{} x{rd}, x{rs1}, {imm}", op.imm_mnemonic()),
            Op { op, rd, rs1, rs2 } => write!(f, "{} x{rd}, x{rs1}, x{rs2}", op.mnemonic()),
            Fence => f.write_str("fence"),
            Ecall => f.write_str("ecall"),
            Ebreak => f.write_str("ebreak"),
            Mret => f.write_str("mret"),
            Csr { op, rd, rs1, csr } => write!(f, "{} x{rd}, {csr:#x}, x{rs1}", op.mnemonic(false)),
            CsrImm { op, rd, zimm, csr } => {
                write!(f, "{} x{rd}, {csr:#x}, {zimm}", op.mnemonic(true))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dis(raw: u32) -> String {
        decode(raw).unwrap().to_string()
    }

    #[test]
    fn decodes_the_fibonacci_program_from_the_cpp_simulator() {
        assert_eq!(dis(0x0010_0093), "addi x1, x0, 1");
        assert_eq!(dis(0x0000_0113), "addi x2, x0, 0");
        assert_eq!(dis(0x0011_01b3), "add x3, x2, x1");
        assert_eq!(dis(0x0010_0133), "add x2, x0, x1");
        assert_eq!(dis(0x0030_00b3), "add x1, x0, x3");
        assert_eq!(dis(0xff5f_f06f), "jal x0, -12");
    }

    #[test]
    fn sra_is_not_confused_with_sub() {
        // Both have funct7 = 0x20; the C++ disassembler printed SRA as SUB.
        assert_eq!(dis(0x4020_d0b3), "sra x1, x1, x2");
        assert_eq!(dis(0x4020_80b3), "sub x1, x1, x2");
        assert_eq!(dis(0x4030_d093), "srai x1, x1, 3");
    }

    #[test]
    fn negative_immediates_are_sign_extended() {
        assert_eq!(dis(0xfff0_0093), "addi x1, x0, -1");
        assert_eq!(dis(0xfe20_8ee3), "beq x1, x2, -4");
        assert_eq!(dis(0xfe11_2e23), "sw x1, -4(x2)");
        assert_eq!(dis(0xffc1_2083), "lw x1, -4(x2)");
    }

    #[test]
    fn decodes_upper_immediates_and_system() {
        assert_eq!(dis(0x1234_50b7), "lui x1, 0x12345");
        assert_eq!(dis(0x0000_0297), "auipc x5, 0x0");
        assert_eq!(dis(0x0000_0073), "ecall");
        assert_eq!(dis(0x3020_0073), "mret");
        assert_eq!(dis(0x3420_2f73), "csrrs x30, 0x342, x0");
        assert_eq!(dis(0x3050_5073), "csrrwi x0, 0x305, 0");
    }

    #[test]
    fn rejects_illegal_encodings() {
        assert_eq!(decode(0x0000_0000), Err(IllegalInstruction(0)));
        assert_eq!(decode(0xffff_ffff), Err(IllegalInstruction(0xffff_ffff)));
        // OP with an unknown funct7.
        assert!(decode(0x0220_80b3 | (0x3f << 25)).is_err());
    }
}
