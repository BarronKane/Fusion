//! RP2350-selected Bluetooth driver activation moved into `fusion-firmware`.
//!
//! `fusion-pal` keeps only the hardware-facing CYW43439 substrate truth. Firmware-layer driver
//! selection, enumeration, and binding now live above PAL where they belong.
