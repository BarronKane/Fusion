# PCU Attribute Macro Vision

## `#[pcu]` — Heterogeneous Compute Dispatch via Proc Macro

```rust
#[pcu(threads = 3)]
fn led_dance(leds: &mut [Led; 8]) {
    // Compiles to PIO instructions on RP2350.
    // 3 state machines run this in parallel.
    // Falls back to green fiber software emulation if no PCU is available.
    // Caller doesn't know which path it took.
}
```

## What the attribute does

1. **Lower** the function body to PCU-IR
2. **Validate** it fits in 32 instructions (PIO instruction memory limit)
3. **Claim** `threads` lanes on a PIO engine with enough capacity at bind time
4. **Generate FIFO feed schedule** if the body references external data
5. **Emit a software fallback** — green fiber path if the PCU isn't available or the body exceeds PIO limits
6. **Wire GPIO pin claims** from component handles (e.g., `Led`) into PINCTRL automatically

## Heterogeneous dispatch

The same PCU-IR can lower to multiple backends:
- **PIO** on RP2350 (9-instruction state machine, cycle-exact)
- **SPIR-V** for Vulkan compute on desktop
- **DXIL** for DX12 compute
- **MSL** for Metal compute

If the function body is expressible in PIO *and* as a compute shader, the same `#[pcu]` attribute targets both. PCU-IR is the pivot point.

This is `#[compute]` with heterogeneous dispatch. Same function, PIO on Pico, compute shader on desktop.

## Component integration

```rust
let led = Led::bind(gpio.claim(15)?, ActiveHigh)?;
let spi = SpiBus::bind(gpio.claim_group([16, 17, 18, 19])?, SpiConfig::default())?;
```

Components carry pin ownership. The `#[pcu]` macro reads pin claims from component handles and configures PINCTRL / pin mappings automatically. The component doesn't know if it's backed by GPIO or PIO — same trait, different driver.

## GPIO abstraction layers

| Layer | Abstraction | Example |
|-------|------------|---------|
| **PAL** | `GpioBase` / `GpioControl` — raw register access, truthful capabilities | "48 pins, these functions available" |
| **Sys** | `Pin<Output>` / `Pin<Alternate<5>>` — type-state ownership, exclusive claim | `gpio.claim(15)?` fails if already claimed |
| **Std** | Components — `Led`, `Button`, `SpiBus` — electrical contract validated at bind | `Led::bind(pin, ActiveHigh)?` — wrong pin type = compile error |

## Key principle

**The consuming crate should never feel like it's bare metal.**

```rust
let led = Led::bind(gpio.claim(15)?, ActiveHigh)?;
led.on();  // Don't care if this is a register write or a PIO program
```
