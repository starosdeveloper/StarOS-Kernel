/* Linker script for StarOS kernel (RISC-V) */

ENTRY(kernel_main)

MEMORY
{
    /* Kernel loaded at 2MB offset (standard for RISC-V) */
    RAM : ORIGIN = 0x80200000, LENGTH = 128M
}

SECTIONS
{
    . = ORIGIN(RAM);
    
    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    } > RAM
    
    .rodata : {
        *(.rodata .rodata.*)
    } > RAM
    
    .data : {
        *(.data .data.*)
    } > RAM
    
    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } > RAM
    
    /DISCARD/ : {
        *(.eh_frame)
        *(.note*)
    }
}
