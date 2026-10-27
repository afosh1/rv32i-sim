OUTPUT_ARCH("riscv")
ENTRY(_start)

SECTIONS
{
  . = 0x80000000;
  .text   : { *(.text.start) *(.text*) }
  .rodata : { *(.rodata*) *(.srodata*) }
  .data   : { *(.data*) *(.sdata*) }
  .bss    : { *(.bss*) *(.sbss*) *(COMMON) }
  . = ALIGN(16);
  . += 0x1000;
  __stack_top = .;
}
