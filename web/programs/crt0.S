# Minimal bare-metal startup: set up the stack, call main, exit(main's result).
    .section .text.start
    .globl _start
_start:
    la    sp, __stack_top
    call  main
    li    a7, 93          # exit syscall number; a0 already holds main's return value
    ecall
