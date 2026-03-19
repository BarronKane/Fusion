/* Pico 2 W (RP2350) memory layout */
MEMORY
{
    /* XIP flash — program code executes in place */
    FLASH : ORIGIN = 0x10000000, LENGTH = 4M
    /* SRAM — all 520KB available to the application */
    RAM   : ORIGIN = 0x20000000, LENGTH = 520K
}
