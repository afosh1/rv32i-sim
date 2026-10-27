# rv32i-sim

[![CI](https://github.com/afosh1/rv32i-sim/actions/workflows/ci.yml/badge.svg)](https://github.com/afosh1/rv32i-sim/actions/workflows/ci.yml)

An RV32I instruction-set simulator in Rust. It runs bare-metal RISC-V ELF
programs, passes **all 42 tests of the official
[riscv-tests](https://github.com/riscv-software-src/riscv-tests) `rv32ui` suite**,
and can dump a **VCD waveform** of a run, so a software execution can be
opened in GTKWave/Surfer right next to an RTL simulation of the same program.

**[Try it in your browser →](https://afosh1.github.io/#demo)** The same code,
compiled to a 50 KB WebAssembly module, steps through a C program, the
original Fibonacci loop and an official riscv-tests binary.

It is a Rust rewrite of the core of my earlier
[C++/Unreal Engine CPU simulator](https://github.com/afosh1/CPUSimulation-RISC-V),
aimed at being a **reference model**: the thing an RTL implementation, such as the
multicycle RISC-V core I designed in SystemVerilog, gets checked against.

```text
$ rv32i-sim rv32ui-p-simple --trace
       1  80000000  0500006f  jal x0, 80
       2  80000050  00000093  addi x1, x0, 0                x1  <- 0x00000000
     ...
      33  800000cc  f1402573  csrrs x10, 0xf14, x0          x10 <- 0x00000000
      37  800000dc  30529073  csrrw x0, 0x305, x5
     ...
      71  80000188  30200073  mret
      72  8000018c  0ff0000f  fence
      73  80000190  00100193  addi x3, x0, 1                x3  <- 0x00000001
      74  80000194  05d00893  addi x17, x0, 93              x17 <- 0x0000005d
      75  80000198  00000513  addi x10, x0, 0               x10 <- 0x00000000
      76  8000019c  00000073  ecall
exit(0) after 76 instructions
```

## Features

- **RV32I** base ISA, plus **Zicsr** and `mret`: enough to run riscv-tests'
  machine-mode boot code unmodified.
- **Typed decoding**: each 32-bit word becomes an `Instruction` enum variant
  carrying only the fields that instruction has (`Branch { kind, rs1, rs2, offset }`).
  The executor matches on meaning, never on raw bit fields.
- **Hand-written ELF32 loader** (PT_LOAD segments, entry point), with a raw-binary fallback.
- **`--trace`**: one line per retired instruction with its register/memory effects.
- **`--vcd out.vcd`**: `clk`, `pc`, `instr` and `x1`–`x31` as a waveform, one clock per instruction.
- **Faults are values, never silent**: out-of-range accesses, illegal encodings and
  misaligned jumps stop the run with the faulting PC (`SimError`); the process exits with 2.
- One dependency (`clap`, for the CLI); the library itself is `std`-only.

## Usage

```sh
cargo build --release
./target/release/rv32i-sim program.elf                 # run; exit code = program's a0
./target/release/rv32i-sim program.elf --trace --regs  # trace + final register file
./target/release/rv32i-sim program.elf --vcd run.vcd   # then: gtkwave run.vcd
```

Programs end by calling `exit` the way riscv-tests and newlib do: `ecall` with
`a7 = 93`, exit code in `a0`. RAM defaults to 1 MiB at `0x8000_0000`
(`--mem-base`, `--mem-size`), and `sp` starts at the top of RAM.

## Verification

| Layer | How |
|---|---|
| Decoder | Unit tests on hand-checked encodings, including negative B/S/J immediates |
| Memory, ELF loader | Unit tests for bounds/overflow faults and malformed files |
| Whole programs | Hand-assembled programs: loops, sign/zero extension, signed vs unsigned ops, faults |
| ISA compliance | **riscv-tests `rv32ui-p-*`: 42/42 pass**, built from a pinned commit in CI |

To run the compliance suite locally, build the tests with any RISC-V GCC:

```sh
git clone --recurse-submodules https://github.com/riscv-software-src/riscv-tests
RISCV_PREFIX=riscv64-unknown-elf- scripts/build-riscv-tests.sh riscv-tests /tmp/rv32ui
RISCV_TESTS_DIR=/tmp/rv32ui cargo test -- --nocapture
```

## What porting from C++ to Rust caught

Rewriting the original C++ simulator surfaced real bugs, each of which Rust either
rejects or makes you spell out:

1. **Signed overflow in `ADD`.** The C++ ALU computed `val1 + val2` on `int32_t`,
   which is undefined behaviour on overflow. RV32 `ADD` wraps; here that is an explicit
   `wrapping_add`, and a test runs Fibonacci past 2³² to prove it.
2. **A bounds check that wrapped.** `if (addr + size > MEMORY_SIZE)` in `uint32_t`
   lets `addr = 0xFFFFFFFE` through, because `0xFFFFFFFE + 4 == 2`. The Rust memory
   uses `checked_sub`/`checked_add` and returns an `AccessFault`.
3. **`SRA` disassembled as `SUB`.** Both have `funct7 = 0x20`, and the C++ disassembler
   checked `funct7` before `funct3`. Decoding once into an enum makes the two
   impossible to confuse.
4. **Left-shifting a negative `int32_t`**, which was undefined before C++20. Registers are `u32`
   here; signedness is chosen per operation (`SLT` vs `SLTU`, `SRA` vs `SRL`).

## Design

```text
 ELF / raw image ──► Memory (base, bytes)
                        │
        ┌───────────── Cpu::step ─────────────┐
        │ fetch ─► decode ─► execute ─► commit│──► Commit { pc, raw, inst, rd_write, mem_write }
        └─────────────────────────────────────┘          │
                                                         ├─► --trace (text)
                                                         └─► --vcd   (waveform)
```

| Module | Responsibility |
|---|---|
| `decode` | `u32` → `Instruction`, plus disassembly via `Display` |
| `cpu` | Architectural state, `step()` (one instruction), `run()` (until `exit`) |
| `memory` | Base-relative little-endian RAM with checked accesses |
| `elf` | ELF32 header and program-header parsing |
| `trace` | Text trace formatting and the streaming `VcdWriter` |
| `web/` | WebAssembly build for the browser demo: a hand-written C ABI (`sim_load`, `sim_step`, `sim_reg`, …), no wasm-bindgen |

The library has no `unsafe`; the only `unsafe` is the `#[unsafe(no_mangle)]` FFI
surface in `web/`. Build the module with:

```sh
cargo build -p rv32i-sim-web --release --target wasm32-unknown-unknown
```

`web/programs/` holds the demo programs and how to rebuild them: `collatz.c`
(freestanding C for RV32I, with its own `crt0.S` and linker script), the raw
Fibonacci loop, and `rv32ui-p-add` from riscv-tests (BSD-3-Clause, see
`LICENSE.riscv-tests`).

## Limitations and roadmap

- **No trap delivery yet.** Exceptions stop the simulator with a diagnostic instead
  of jumping to `mtvec`, so the `rv32mi` privileged tests are out of scope for now.
- **No M/A/F/C extensions**, and no timing model: one instruction = one clock.
- Next: the M extension; trap delivery; and lockstep co-simulation against my
  SystemVerilog core (Verilator), comparing commits instruction by instruction.

## License

MIT
