/* RISC-V 64-bit Linker Script for StarOS Kernel */

OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    RAM : ORIGIN = 0x80000000, LENGTH = 512M
}

SECTIONS
{
    . = 0x80000000;
    
    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    } > RAM
    
    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    } > RAM
    
    .data : ALIGN(4K) {
        *(.data .data.*)
    } > RAM
    
    .bss : ALIGN(4K) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } > RAM
    
    . = ALIGN(4K);
    __kernel_end = .;
    
    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
    }
}
