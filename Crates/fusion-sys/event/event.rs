//! fusion-sys-level event wrappers built on top of fusion-pal-truthful backends.
//!
//! `fusion-sys::event` is the narrow policy-free layer above the fusion-pal event contracts. It
//! keeps the readiness-vs-completion distinction intact and exposes selected backend
//! pollers without pretending different kernel event models are secretly identical.

use core::time::Duration;

pub use fusion_pal::sys::event::{
    EventBase,
    EventCaps,
    EventCompletion,
    EventCompletionOp,
    EventCompletionOpKind,
    EventError,
    EventErrorKind,
    EventImplementationKind,
    EventInterest,
    EventKey,
    EventModel,
    EventNotification,
    EventReadiness,
    EventRecord,
    EventRegistration,
    EventRegistrationMode,
    EventSource,
    EventSourceHandle,
    EventSupport,
};
use fusion_pal::sys::event::{PlatformEvent, PlatformPoller, system_event as pal_system_event};

/// fusion-sys event provider wrapper around the selected fusion-pal backend.
#[derive(Debug, Clone, Copy)]
pub struct EventSystem {
    inner: PlatformEvent,
}

/// Owned poller handle for the selected backend.
#[derive(Debug)]
pub struct EventPoller {
    inner: PlatformPoller,
}

impl EventSystem {
    /// Creates a wrapper for the selected platform event provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_event(),
        }
    }

    /// Reports the truthful event surface for the selected backend.
    #[must_use]
    pub fn support(&self) -> EventSupport {
        EventBase::support(&self.inner)
    }

    /// Creates a new backend poller instance.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported polling surfaces.
    pub fn create(&self) -> Result<EventPoller, EventError> {
        let poller = EventSource::create(&self.inner)?;
        Ok(EventPoller { inner: poller })
    }

    /// Registers a source with the backend poller.
    ///
    /// # Errors
    ///
    /// Returns any honest backend registration failure.
    pub fn register(
        &self,
        poller: &mut EventPoller,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        EventSource::register(&self.inner, &mut poller.inner, source, interest)
    }

    /// Registers a source with an explicit delivery policy.
    ///
    /// # Errors
    ///
    /// Returns any honest backend registration failure.
    pub fn register_with(
        &self,
        poller: &mut EventPoller,
        registration: EventRegistration,
    ) -> Result<EventKey, EventError> {
        EventSource::register_with(&self.inner, &mut poller.inner, registration)
    }

    /// Updates an existing registration.
    ///
    /// # Errors
    ///
    /// Returns any honest backend re-registration failure.
    pub fn reregister(
        &self,
        poller: &mut EventPoller,
        key: EventKey,
        interest: EventInterest,
    ) -> Result<(), EventError> {
        EventSource::reregister(&self.inner, &mut poller.inner, key, interest)
    }

    /// Updates an existing registration with an explicit delivery policy.
    ///
    /// # Errors
    ///
    /// Returns any honest backend re-registration failure.
    pub fn reregister_with(
        &self,
        poller: &mut EventPoller,
        key: EventKey,
        registration: EventRegistration,
    ) -> Result<(), EventError> {
        EventSource::reregister_with(&self.inner, &mut poller.inner, key, registration)
    }

    /// Removes an existing registration.
    ///
    /// # Errors
    ///
    /// Returns any honest backend deregistration failure.
    pub fn deregister(&self, poller: &mut EventPoller, key: EventKey) -> Result<(), EventError> {
        EventSource::deregister(&self.inner, &mut poller.inner, key)
    }

    /// Submits a completion-style operation when the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported completion submission.
    pub fn submit(
        &self,
        poller: &mut EventPoller,
        operation: EventCompletionOp,
    ) -> Result<EventKey, EventError> {
        EventSource::submit(&self.inner, &mut poller.inner, operation)
    }

    /// Polls the backend for ready or completed events.
    ///
    /// # Errors
    ///
    /// Returns any honest backend polling failure.
    pub fn poll(
        &self,
        poller: &mut EventPoller,
        events: &mut [EventRecord],
        timeout: Option<Duration>,
    ) -> Result<usize, EventError> {
        EventSource::poll(&self.inner, &mut poller.inner, events, timeout)
    }
}

impl Default for EventSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
/// Cortex-M-specific event source helpers.
pub mod cortex_m {
    use super::{EventInterest, EventRegistration, EventRegistrationMode, EventSourceHandle};
    #[cfg(feature = "soc-rp2350")]
    use fusion_pal::sys::soc::cortex_m::hal::soc::board::{
        CortexMDmaRequestClass,
        CortexMDmaRequestDescriptor,
        CortexMDmaTransferCaps,
        dma_requests as rp2350_dma_requests,
        gpio_irq_clear_edges as rp2350_gpio_irq_clear_edges,
        gpio_irq_summary as rp2350_gpio_irq_summary,
        pio_irq_clear_internal_flags as rp2350_pio_irq_clear_internal_flags,
        pio_irq_summary as rp2350_pio_irq_summary,
        spi_irq_acknowledge_clearable as rp2350_spi_irq_acknowledge_clearable,
        spi_irq_summary as rp2350_spi_irq_summary,
    };
    #[cfg(feature = "soc-rp2350")]
    pub use fusion_pal::sys::soc::cortex_m::hal::soc::board::{
        Rp2350GpioIrqSummary,
        Rp2350PioIrqSummary,
        Rp2350SpiIrqSummary,
    };

    /// Typed wrapper for one Cortex-M external IRQ line used as an event source.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CortexMIrqSource {
        irqn: u16,
    }

    impl CortexMIrqSource {
        /// Creates a typed Cortex-M IRQ event source.
        #[must_use]
        pub const fn new(irqn: u16) -> Self {
            Self { irqn }
        }

        /// Returns the backing NVIC interrupt number.
        #[must_use]
        pub const fn irqn(self) -> u16 {
            self.irqn
        }

        /// Returns the backend-neutral event source handle.
        #[must_use]
        pub const fn handle(self) -> EventSourceHandle {
            EventSourceHandle(self.irqn as usize)
        }

        /// Builds a full event registration for this IRQ line.
        #[must_use]
        pub const fn registration(
            self,
            interest: EventInterest,
            mode: EventRegistrationMode,
        ) -> EventRegistration {
            EventRegistration {
                source: self.handle(),
                interest,
                mode,
            }
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 timer-alarm event source helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Rp2350TimerAlarmSource {
        /// `TIMER0_IRQ_0`
        Timer0Alarm0,
        /// `TIMER0_IRQ_1`
        Timer0Alarm1,
        /// `TIMER0_IRQ_2`
        Timer0Alarm2,
        /// `TIMER0_IRQ_3`
        Timer0Alarm3,
        /// `TIMER1_IRQ_0`
        Timer1Alarm0,
        /// `TIMER1_IRQ_1`
        Timer1Alarm1,
        /// `TIMER1_IRQ_2`
        Timer1Alarm2,
        /// `TIMER1_IRQ_3`
        Timer1Alarm3,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350TimerAlarmSource {
        /// Returns the typed Cortex-M IRQ source for this timer alarm.
        #[must_use]
        pub const fn source(self) -> CortexMIrqSource {
            CortexMIrqSource::new(match self {
                Self::Timer0Alarm0 => 0,
                Self::Timer0Alarm1 => 1,
                Self::Timer0Alarm2 => 2,
                Self::Timer0Alarm3 => 3,
                Self::Timer1Alarm0 => 4,
                Self::Timer1Alarm1 => 5,
                Self::Timer1Alarm2 => 6,
                Self::Timer1Alarm3 => 7,
            })
        }

        /// Returns the recommended registration for one timer-alarm readiness source.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.source().registration(
                EventInterest::READABLE,
                EventRegistrationMode::LevelAckOnPoll,
            )
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 DMA IRQ-group event source helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Rp2350DmaIrqSource {
        /// `DMA_IRQ_0`
        Irq0,
        /// `DMA_IRQ_1`
        Irq1,
        /// `DMA_IRQ_2`
        Irq2,
        /// `DMA_IRQ_3`
        Irq3,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350DmaIrqSource {
        /// Returns the typed Cortex-M IRQ source for this DMA IRQ group.
        #[must_use]
        pub const fn source(self) -> CortexMIrqSource {
            CortexMIrqSource::new(match self {
                Self::Irq0 => 10,
                Self::Irq1 => 11,
                Self::Irq2 => 12,
                Self::Irq3 => 13,
            })
        }

        /// Returns the recommended registration for one DMA IRQ group.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.source().registration(
                EventInterest::READABLE,
                EventRegistrationMode::LevelAckOnPoll,
            )
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 DMA channel helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Rp2350DmaChannel(u8);

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350DmaChannel {
        /// Total number of hardware DMA channels surfaced by RP2350.
        pub const COUNT: u8 = 16;

        /// Creates a typed DMA channel selector when the index is valid.
        #[must_use]
        pub const fn new(index: u8) -> Option<Self> {
            if index < Self::COUNT {
                Some(Self(index))
            } else {
                None
            }
        }

        /// Returns the zero-based hardware channel index.
        #[must_use]
        pub const fn index(self) -> u8 {
            self.0
        }

        /// Returns the DMA IRQ group that reports completion for this channel.
        #[must_use]
        pub const fn irq_source(self) -> Rp2350DmaIrqSource {
            match self.0 / 4 {
                0 => Rp2350DmaIrqSource::Irq0,
                1 => Rp2350DmaIrqSource::Irq1,
                2 => Rp2350DmaIrqSource::Irq2,
                _ => Rp2350DmaIrqSource::Irq3,
            }
        }

        /// Returns the recommended registration for this channel's completion group.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.irq_source().registration()
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 DMA request helper over the selected board descriptor table.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Rp2350DmaRequest {
        descriptor: &'static CortexMDmaRequestDescriptor,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350DmaRequest {
        /// Returns all surfaced RP2350 DMA request descriptors.
        #[must_use]
        pub fn all() -> &'static [CortexMDmaRequestDescriptor] {
            rp2350_dma_requests()
        }

        /// Looks up one DMA request by its hardware request-line selector.
        #[must_use]
        pub fn from_request_line(request_line: u16) -> Option<Self> {
            Self::all()
                .iter()
                .find(|descriptor| descriptor.request_line == request_line)
                .map(|descriptor| Self { descriptor })
        }

        /// Returns the underlying board descriptor for this DMA request.
        #[must_use]
        pub const fn descriptor(self) -> &'static CortexMDmaRequestDescriptor {
            self.descriptor
        }

        /// Returns the hardware request-line selector.
        #[must_use]
        pub const fn request_line(self) -> u16 {
            self.descriptor.request_line
        }

        /// Returns the coarse request class.
        #[must_use]
        pub const fn class(self) -> CortexMDmaRequestClass {
            self.descriptor.class
        }

        /// Returns the associated peripheral block when one exists.
        #[must_use]
        pub const fn peripheral(self) -> Option<&'static str> {
            self.descriptor.peripheral
        }

        /// Returns the peripheral-local endpoint name when one exists.
        #[must_use]
        pub const fn endpoint(self) -> Option<&'static str> {
            self.descriptor.endpoint
        }

        /// Returns the coarse transfer capabilities for this request.
        #[must_use]
        pub const fn transfer_caps(self) -> CortexMDmaTransferCaps {
            self.descriptor.transfer_caps
        }

        /// Returns the completion IRQ registration recommended for a specific DMA channel.
        #[must_use]
        pub const fn registration_for_channel(
            self,
            channel: Rp2350DmaChannel,
        ) -> EventRegistration {
            let _ = self;
            channel.registration()
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 GPIO-bank IRQ event source helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Rp2350GpioIrqSource {
        /// `IO_IRQ_BANK0`
        Bank0,
        /// `IO_IRQ_BANK0_NS`
        Bank0NonSecure,
        /// `IO_IRQ_QSPI`
        Qspi,
        /// `IO_IRQ_QSPI_NS`
        QspiNonSecure,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350GpioIrqSource {
        /// Returns the typed Cortex-M IRQ source for this GPIO-bank IRQ.
        #[must_use]
        pub const fn source(self) -> CortexMIrqSource {
            CortexMIrqSource::new(match self {
                Self::Bank0 => 21,
                Self::Bank0NonSecure => 22,
                Self::Qspi => 23,
                Self::QspiNonSecure => 24,
            })
        }

        /// Returns the recommended registration for one GPIO-bank IRQ.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.source()
                .registration(EventInterest::READABLE, EventRegistrationMode::LevelSticky)
        }

        /// Returns the current raw shared-summary snapshot for this GPIO IRQ source.
        ///
        /// This is the honest driver-local path for GPIO summary lines. It does not pretend the
        /// PAL can generically acknowledge one GPIO bank without knowing which edge bits the
        /// driver actually wants cleared.
        ///
        /// # Errors
        ///
        /// Returns any honest board-local failure while reading the current bank summary.
        pub fn pending_summary(self) -> Result<Rp2350GpioIrqSummary, super::EventError> {
            rp2350_gpio_irq_summary(self.source().irqn()).map_err(map_rp2350_hardware_error)
        }

        /// Clears one raw edge-event mask in the selected GPIO summary word.
        ///
        /// `word_index` is bank-local, and `edge_mask` uses the raw `INTRx` nibble layout.
        ///
        /// # Errors
        ///
        /// Returns an error when the word index is invalid for this bank or the board rejects the
        /// request.
        pub fn clear_edge_mask(
            self,
            word_index: usize,
            edge_mask: u32,
        ) -> Result<(), super::EventError> {
            rp2350_gpio_irq_clear_edges(self.source().irqn(), word_index, edge_mask)
                .map_err(map_rp2350_hardware_error)
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 PIO IRQ event source helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Rp2350PioIrqSource {
        /// `PIO0_IRQ_0`
        Pio0Irq0,
        /// `PIO0_IRQ_1`
        Pio0Irq1,
        /// `PIO1_IRQ_0`
        Pio1Irq0,
        /// `PIO1_IRQ_1`
        Pio1Irq1,
        /// `PIO2_IRQ_0`
        Pio2Irq0,
        /// `PIO2_IRQ_1`
        Pio2Irq1,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350PioIrqSource {
        /// Returns the typed Cortex-M IRQ source for this PIO IRQ.
        #[must_use]
        pub const fn source(self) -> CortexMIrqSource {
            CortexMIrqSource::new(match self {
                Self::Pio0Irq0 => 15,
                Self::Pio0Irq1 => 16,
                Self::Pio1Irq0 => 17,
                Self::Pio1Irq1 => 18,
                Self::Pio2Irq0 => 19,
                Self::Pio2Irq1 => 20,
            })
        }

        /// Returns the recommended registration for one PIO IRQ.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.source()
                .registration(EventInterest::READABLE, EventRegistrationMode::LevelSticky)
        }

        /// Returns the current raw shared-summary snapshot for this PIO IRQ source.
        ///
        /// # Errors
        ///
        /// Returns any honest board-local failure while reading the current PIO summary bits.
        pub fn pending_summary(self) -> Result<Rp2350PioIrqSummary, super::EventError> {
            rp2350_pio_irq_summary(self.source().irqn()).map_err(map_rp2350_hardware_error)
        }

        /// Clears the internal `PIO_IRQ` flag byte for this PIO source.
        ///
        /// This does not claim FIFO threshold conditions are clearable, because they are not.
        ///
        /// # Errors
        ///
        /// Returns an error when the board rejects the requested clear.
        pub fn clear_internal_irq_flags(self, flags: u8) -> Result<(), super::EventError> {
            rp2350_pio_irq_clear_internal_flags(self.source().irqn(), flags)
                .map_err(map_rp2350_hardware_error)
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 UART IRQ event source helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Rp2350UartIrqSource {
        /// `UART0_IRQ`
        Uart0,
        /// `UART1_IRQ`
        Uart1,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350UartIrqSource {
        /// Returns the typed Cortex-M IRQ source for this UART IRQ.
        #[must_use]
        pub const fn source(self) -> CortexMIrqSource {
            CortexMIrqSource::new(match self {
                Self::Uart0 => 33,
                Self::Uart1 => 34,
            })
        }

        /// Returns the recommended registration for one UART IRQ.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.source().registration(
                EventInterest::READABLE,
                EventRegistrationMode::LevelAckOnPoll,
            )
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 SPI IRQ event source helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Rp2350SpiIrqSource {
        /// `SPI0_IRQ`
        Spi0,
        /// `SPI1_IRQ`
        Spi1,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350SpiIrqSource {
        /// Returns the typed Cortex-M IRQ source for this SPI IRQ.
        #[must_use]
        pub const fn source(self) -> CortexMIrqSource {
            CortexMIrqSource::new(match self {
                Self::Spi0 => 31,
                Self::Spi1 => 32,
            })
        }

        /// Returns the recommended registration for one SPI IRQ.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.source()
                .registration(EventInterest::READABLE, EventRegistrationMode::LevelSticky)
        }

        /// Returns the current raw shared-summary snapshot for this SPI IRQ source.
        ///
        /// # Errors
        ///
        /// Returns any honest board-local failure while reading the current SPI summary bits.
        pub fn pending_summary(self) -> Result<Rp2350SpiIrqSummary, super::EventError> {
            rp2350_spi_irq_summary(self.source().irqn()).map_err(map_rp2350_hardware_error)
        }

        /// Acknowledges the clearable RT/ROR causes for this SPI IRQ source.
        ///
        /// The returned bitmask contains the clearable causes that were actually acknowledged.
        ///
        /// # Errors
        ///
        /// Returns an error when the board rejects the requested clear.
        pub fn acknowledge_clearable(self) -> Result<u8, super::EventError> {
            rp2350_spi_irq_acknowledge_clearable(self.source().irqn())
                .map_err(map_rp2350_hardware_error)
        }
    }

    #[cfg(feature = "soc-rp2350")]
    /// Typed RP2350 I2C IRQ event source helper.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Rp2350I2cIrqSource {
        /// `I2C0_IRQ`
        I2c0,
        /// `I2C1_IRQ`
        I2c1,
    }

    #[cfg(feature = "soc-rp2350")]
    impl Rp2350I2cIrqSource {
        /// Returns the typed Cortex-M IRQ source for this I2C IRQ.
        #[must_use]
        pub const fn source(self) -> CortexMIrqSource {
            CortexMIrqSource::new(match self {
                Self::I2c0 => 36,
                Self::I2c1 => 37,
            })
        }

        /// Returns the recommended registration for one I2C IRQ.
        #[must_use]
        pub const fn registration(self) -> EventRegistration {
            self.source().registration(
                EventInterest::READABLE,
                EventRegistrationMode::LevelAckOnPoll,
            )
        }
    }

    #[cfg(feature = "soc-rp2350")]
    const fn map_rp2350_hardware_error(
        error: fusion_pal::contract::pal::HardwareError,
    ) -> super::EventError {
        use fusion_pal::contract::pal::HardwareErrorKind;

        match error.kind() {
            HardwareErrorKind::Unsupported => super::EventError::unsupported(),
            HardwareErrorKind::Invalid => super::EventError::invalid(),
            HardwareErrorKind::Busy => super::EventError::busy(),
            HardwareErrorKind::ResourceExhausted => super::EventError::resource_exhausted(),
            HardwareErrorKind::StateConflict => super::EventError::state_conflict(),
            HardwareErrorKind::Platform(code) => super::EventError::platform(code),
        }
    }
}
