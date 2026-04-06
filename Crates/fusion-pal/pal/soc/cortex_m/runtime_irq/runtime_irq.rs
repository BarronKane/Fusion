//! Cortex-M backend-owned reserved runtime IRQ dispatch.
//!
//! This keeps backend wake machinery out of examples. When Fusion does not yet own the adopted
//! vector table, the default interrupt path still needs to service reserved runtime lines like the
//! shared timeout alarm.

/// Backend-owned fallback IRQ dispatch for reserved runtime lines.
#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
#[unsafe(no_mangle)]
unsafe extern "C" fn DefaultHandler(irqn: i16) {
    match crate::pal::soc::cortex_m::hal::soc::board::service_reserved_runtime_irq(irqn) {
        Ok(true) => return,
        Ok(false) | Err(_) => {}
    }

    panic!("unhandled Cortex-M IRQ reached backend DefaultHandler");
}
