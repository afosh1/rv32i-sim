//! Whole-program tests on hand-assembled RV32I code.

use rv32i_sim::{Cpu, SimError, load_program};

const BASE: u32 = 0x8000_0000;

// --- A minimal encoder, so programs below read like assembly -------------------

const A0: u32 = 10;
const A7: u32 = 17;
const T0: u32 = 5;

fn r_type(funct7: u32, rs2: u32, rs1: u32, funct3: u32, rd: u32) -> u32 {
    funct7 << 25 | rs2 << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | 0x33
}

fn i_type(imm: i32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    ((imm as u32) << 20) | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn b_type(imm: i32, rs2: u32, rs1: u32, funct3: u32) -> u32 {
    let imm = imm as u32;
    (imm >> 12 & 1) << 31
        | (imm >> 5 & 0x3f) << 25
        | rs2 << 20
        | rs1 << 15
        | funct3 << 12
        | (imm >> 1 & 0xf) << 8
        | (imm >> 11 & 1) << 7
        | 0x63
}

fn add(rd: u32, rs1: u32, rs2: u32) -> u32 {
    r_type(0, rs2, rs1, 0, rd)
}
fn sub(rd: u32, rs1: u32, rs2: u32) -> u32 {
    r_type(0x20, rs2, rs1, 0, rd)
}
fn sra(rd: u32, rs1: u32, rs2: u32) -> u32 {
    r_type(0x20, rs2, rs1, 5, rd)
}
fn srl(rd: u32, rs1: u32, rs2: u32) -> u32 {
    r_type(0, rs2, rs1, 5, rd)
}
fn slt(rd: u32, rs1: u32, rs2: u32) -> u32 {
    r_type(0, rs2, rs1, 2, rd)
}
fn sltu(rd: u32, rs1: u32, rs2: u32) -> u32 {
    r_type(0, rs2, rs1, 3, rd)
}
fn addi(rd: u32, rs1: u32, imm: i32) -> u32 {
    i_type(imm, rs1, 0, rd, 0x13)
}
fn lb(rd: u32, rs1: u32, imm: i32) -> u32 {
    i_type(imm, rs1, 0, rd, 0x03)
}
fn lbu(rd: u32, rs1: u32, imm: i32) -> u32 {
    i_type(imm, rs1, 4, rd, 0x03)
}
fn sw(rs2: u32, rs1: u32, imm: i32) -> u32 {
    let imm = imm as u32;
    (imm >> 5 & 0x7f) << 25 | rs2 << 20 | rs1 << 15 | 2 << 12 | (imm & 0x1f) << 7 | 0x23
}
fn bne(rs1: u32, rs2: u32, offset: i32) -> u32 {
    b_type(offset, rs2, rs1, 1)
}
fn jalr(rd: u32, rs1: u32, imm: i32) -> u32 {
    i_type(imm, rs1, 0, rd, 0x67)
}
const ECALL: u32 = 0x73;

fn boot(program: &[u32]) -> Cpu {
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    load_program(&bytes, BASE, 4096).unwrap()
}

/// Run until exit and return a0.
fn run(program: &[u32]) -> u32 {
    boot(program).run(10_000, |_, _| {}).unwrap().code
}

// --- Tests -----------------------------------------------------------------------

#[test]
fn sum_one_to_ten_and_exit() {
    let code = run(&[
        addi(A0, 0, 0),
        addi(T0, 0, 10),
        add(A0, A0, T0), // loop:
        addi(T0, T0, -1),
        bne(T0, 0, -8), // -> loop
        addi(A7, 0, 93),
        ECALL,
    ]);
    assert_eq!(code, 55);
}

/// The exact program from the original C++ simulator (Programs.cpp).
const FIBONACCI: [u32; 6] = [
    0x0010_0093,
    0x0000_0113,
    0x0011_01b3,
    0x0010_0133,
    0x0030_00b3,
    0xff5f_f06f,
];

fn fibonacci_after(iterations: u64) -> u32 {
    let mut cpu = boot(&FIBONACCI);
    for _ in 0..2 + 4 * iterations {
        cpu.step().unwrap();
    }
    cpu.reg(1)
}

#[test]
fn fibonacci_from_the_cpp_simulator() {
    assert_eq!(fibonacci_after(10), 89); // F(11)
}

#[test]
fn fibonacci_wraps_like_hardware() {
    // F(48) = 4_807_526_976 does not fit in 32 bits; RV32 ADD keeps the low 32.
    // (The C++ version did this in `int32_t`, where overflow is undefined behaviour.)
    assert_eq!(fibonacci_after(47), (4_807_526_976u64 % (1 << 32)) as u32);
}

#[test]
fn signedness_lives_in_the_operation() {
    let code = run(&[
        addi(T0, 0, -8), // t0 = 0xFFFF_FFF8
        addi(6, 0, 1),   // t1 = 1
        sra(7, T0, 6),   // t2 = -4          (arithmetic: sign bit copied in)
        srl(28, T0, 6),  // t3 = 0x7FFF_FFFC (logical: zero shifted in)
        slt(29, T0, 0),  // t4 = 1           (-8 < 0 signed)
        sltu(30, T0, 0), // t5 = 0           (huge unsigned is not < 0)
        sub(A0, 7, 28),  // a0 = t2 - t3
        add(A0, A0, 29), // + t4
        add(A0, A0, 30), // + t5
        addi(A7, 0, 93),
        ECALL,
    ]);
    let expected = (-4i32 as u32).wrapping_sub(0x7fff_fffc).wrapping_add(1);
    assert_eq!(code, expected);
}

#[test]
fn loads_sign_and_zero_extend() {
    let code = run(&[
        addi(T0, 0, 0xff), // t0 = 0xFF
        addi(6, 2, -16),   // t1 = sp - 16
        sw(T0, 6, 0),      // mem[t1] = 0x000000FF
        lb(7, 6, 0),       // t2 = -1   (sign-extended)
        lbu(28, 6, 0),     // t3 = 255  (zero-extended)
        add(A0, 7, 28),    // a0 = 254
        addi(A7, 0, 93),
        ECALL,
    ]);
    assert_eq!(code, 254);
}

#[test]
fn x0_ignores_writes() {
    let code = run(&[addi(0, 0, 42), add(A0, 0, 0), addi(A7, 0, 93), ECALL]);
    assert_eq!(code, 0);
}

#[test]
fn reports_faults_with_their_pc() {
    let mut cpu = boot(&[jalr(0, 0, 2)]); // jump to address 2
    assert_eq!(
        cpu.run(10, |_, _| {}),
        Err(SimError::MisalignedJump {
            pc: BASE,
            target: 2
        })
    );

    let mut cpu = boot(&[0xffff_ffff]);
    assert_eq!(
        cpu.run(10, |_, _| {}),
        Err(SimError::IllegalInstruction {
            pc: BASE,
            raw: 0xffff_ffff
        })
    );

    let mut cpu = boot(&[lb(A0, 0, 0)]); // address 0 is below RAM
    assert!(matches!(
        cpu.run(10, |_, _| {}),
        Err(SimError::LoadFault { pc: BASE, .. })
    ));

    let mut cpu = boot(&[FIBONACCI[5]; 1]); // jal x0, -12: jumps below RAM
    assert!(matches!(
        cpu.run(10, |_, _| {}),
        Err(SimError::FetchFault { .. })
    ));
}

#[test]
fn infinite_loops_hit_the_step_limit() {
    let mut cpu = boot(&[0x0000_006f]); // jal x0, 0
    assert_eq!(
        cpu.run(1_000, |_, _| {}),
        Err(SimError::StepLimit { steps: 1_000 })
    );
}
