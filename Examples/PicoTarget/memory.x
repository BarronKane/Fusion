/* Pico 2 W (RP2350) memory layout */
MEMORY
{
    /* XIP flash — program code executes in place */
    FLASH : ORIGIN = 0x10000000, LENGTH = 4M
    /* SRAM — all 520KB available to the application */
    RAM   : ORIGIN = 0x20000000, LENGTH = 520K
}

/* Reserve a fixed 64 KiB stack at the top of RAM.
 * The gap between `__sheap` (set by cortex-m-rt after .bss/.uninit) and `_stack_end`
 * becomes board-owned free SRAM that the Cortex-M PAL can surface honestly.
 */
_stack_end = ORIGIN(RAM) + LENGTH(RAM) - 64K;
