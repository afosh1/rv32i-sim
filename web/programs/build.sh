#!/usr/bin/env bash
# Rebuild the demo programs embedded in the WebAssembly module.
#
#   web/programs/build.sh <rv32ui build dir from scripts/build-riscv-tests.sh>
#
# RISCV_PREFIX selects the toolchain (default riscv64-unknown-elf-).
set -euo pipefail
cd "$(dirname "$0")"
prefix="${RISCV_PREFIX:-riscv64-unknown-elf-}"

# collatz.elf: freestanding C, RV32I only (no M extension, so no mul/div).
"${prefix}gcc" -march=rv32i -mabi=ilp32 -O2 -ffreestanding -nostdlib -mno-relax \
  -Wl,--no-warn-rwx-segments -T link.ld crt0.S collatz.c -o collatz.elf

# fibonacci.bin: the hand-assembled loop from the original C++ simulator.
printf '\x93\x00\x10\x00\x13\x01\x00\x00\xb3\x01\x11\x00\x33\x01\x10\x00\xb3\x00\x30\x00\x6f\xf0\x5f\xff' > fibonacci.bin

# rv32ui-p-add: one official riscv-tests binary (BSD-3-Clause, see riscv-tests/LICENSE).
cp "${1:?usage: $0 <rv32ui build dir>}/rv32ui-p-add" .

ls -l collatz.elf fibonacci.bin rv32ui-p-add
