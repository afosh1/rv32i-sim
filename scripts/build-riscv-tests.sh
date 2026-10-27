#!/usr/bin/env bash
# Build the official riscv-tests rv32ui "p" suite (bare metal, physical memory).
#
#   scripts/build-riscv-tests.sh <riscv-tests checkout> <output dir>
#
# RISCV_PREFIX selects the toolchain, e.g. riscv64-unknown-elf- (Ubuntu's
# gcc-riscv64-unknown-elf, the default) or riscv-none-elf- (xPack, Windows/macOS).
set -euo pipefail

src="${1:?usage: $0 <riscv-tests dir> <out dir>}"
out="${2:?usage: $0 <riscv-tests dir> <out dir>}"
prefix="${RISCV_PREFIX:-riscv64-unknown-elf-}"

mkdir -p "$out"
for test in "$src"/isa/rv32ui/*.S; do
  name="rv32ui-p-$(basename "$test" .S)"
  "${prefix}gcc" -march=rv32i_zicsr_zifencei -mabi=ilp32 \
    -static -mcmodel=medany -fvisibility=hidden -nostdlib -nostartfiles \
    -I"$src/env/p" -I"$src/isa/macros/scalar" -T"$src/env/p/link.ld" \
    "$test" -o "$out/$name"
done
echo "built $(find "$out" -name 'rv32ui-p-*' | wc -l) tests into $out"
