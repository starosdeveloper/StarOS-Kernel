/* Linker script for Samsung Exynos SoCs */
/* Load address: 0x80000000 (typical for Exynos) */

OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH(aarch64)
ENTRY(_start)

MEMORY
{
    RAM : ORIGIN = 0x80000000, LENGTH = 128M
}

SECTIONS
{
    . = 0x80000000;

    .text : ALIGN(4K)
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    } > RAM

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    } > RAM

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    } > RAM

    .bss : ALIGN(4K)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } > RAM

    .stack : ALIGN(4K)
    {
        . = . + 0x10000; /* 64KB stack */
        __stack_top = .;
    } > RAM

    __kernel_end = .;

    /DISCARD/ :
    {
        *(.comment)
        *(.gnu*)
        *(.note*)
        *(.eh_frame*)
    }
}
