ENTRY(_start)

SECTIONS
{
    . = 0x40080000;
    
    .text : {
        *(.text.vectors)
        *(.text._start)
        *(.text*)
    }
    
    .rodata : {
        *(.rodata*)
    }
    
    .data : {
        *(.data*)
    }
    
    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss*)
        *(COMMON)
        __bss_end = .;
    }
    
    /DISCARD/ : {
        *(.eh_frame*)
    }
}
