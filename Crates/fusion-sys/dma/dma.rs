//! fusion-sys DMA descriptors and consumer-side policy helpers.

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
/// Cortex-M DMA helpers over the selected board descriptor tables.
pub mod cortex_m {
    pub use fusion_pal::sys::soc::cortex_m::hal::soc::board::{
        CortexMDmaControllerDescriptor,
        CortexMDmaRequestClass,
        CortexMDmaRequestDescriptor,
        CortexMDmaTransferCaps,
        dma_controllers as selected_dma_controllers,
        dma_requests as selected_dma_requests,
    };

    /// Recommended transfer shape for one DMA request consumer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum CortexMDmaTransferShape {
        /// A memory-to-memory transfer path.
        MemoryToMemory,
        /// A memory-to-peripheral transfer path.
        MemoryToPeripheral,
        /// A peripheral-to-memory transfer path.
        PeripheralToMemory,
        /// One channel-to-channel chaining or trigger path.
        ChannelChaining,
    }

    /// Consumer-facing role inferred from one DMA request descriptor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum CortexMDmaConsumerRole {
        /// Peripheral transmit or drain pacing.
        PeripheralTx,
        /// Peripheral receive or fill pacing.
        PeripheralRx,
        /// Peripheral-generated pacing that is not plain TX/RX FIFO traffic.
        PeripheralPacer,
        /// Timer-driven pacing.
        TimerPacer,
        /// Software-forced request with no peripheral endpoint.
        Force,
    }

    /// Consumer-side routing and pacing policy for one DMA request.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CortexMDmaRequestPolicy {
        /// Coarse consumer role of this request.
        pub role: CortexMDmaConsumerRole,
        /// Preferred transfer shape when one is implied by the request.
        pub preferred_shape: Option<CortexMDmaTransferShape>,
    }

    /// Typed helper over one selected Cortex-M DMA request descriptor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CortexMDmaRequest {
        descriptor: &'static CortexMDmaRequestDescriptor,
    }

    impl CortexMDmaRequest {
        /// Returns all surfaced DMA request descriptors for the selected Cortex-M board.
        #[must_use]
        pub fn all() -> &'static [CortexMDmaRequestDescriptor] {
            selected_dma_requests()
        }

        /// Returns all surfaced DMA controller descriptors for the selected Cortex-M board.
        #[must_use]
        pub fn controllers() -> &'static [CortexMDmaControllerDescriptor] {
            selected_dma_controllers()
        }

        /// Looks up one DMA request by its hardware request-line selector.
        #[must_use]
        pub fn from_request_line(request_line: u16) -> Option<Self> {
            Self::all()
                .iter()
                .find(|descriptor| descriptor.request_line == request_line)
                .map(|descriptor| Self { descriptor })
        }

        /// Returns the underlying descriptor.
        #[must_use]
        pub const fn descriptor(self) -> &'static CortexMDmaRequestDescriptor {
            self.descriptor
        }

        /// Returns the hardware request-line selector.
        #[must_use]
        pub const fn request_line(self) -> u16 {
            self.descriptor.request_line
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

        /// Returns the coarse request classification.
        #[must_use]
        pub const fn class(self) -> CortexMDmaRequestClass {
            self.descriptor.class
        }

        /// Returns the transfer capability envelope of this request.
        #[must_use]
        pub const fn transfer_caps(self) -> CortexMDmaTransferCaps {
            self.descriptor.transfer_caps
        }

        /// Returns the consumer-facing routing and pacing policy for this request.
        #[must_use]
        pub const fn policy(self) -> CortexMDmaRequestPolicy {
            policy_for_descriptor(self.descriptor)
        }

        /// Returns the consumer-facing role inferred for this request.
        #[must_use]
        pub const fn consumer_role(self) -> CortexMDmaConsumerRole {
            self.policy().role
        }

        /// Returns the preferred transfer shape implied by this request, when one exists.
        #[must_use]
        pub const fn preferred_shape(self) -> Option<CortexMDmaTransferShape> {
            self.policy().preferred_shape
        }

        /// Returns whether the descriptor honestly supports the requested transfer shape.
        #[must_use]
        pub const fn supports_shape(self, shape: CortexMDmaTransferShape) -> bool {
            let caps = self.transfer_caps();
            match shape {
                CortexMDmaTransferShape::MemoryToMemory => {
                    caps.contains(CortexMDmaTransferCaps::MEMORY_TO_MEMORY)
                }
                CortexMDmaTransferShape::MemoryToPeripheral => {
                    caps.contains(CortexMDmaTransferCaps::MEMORY_TO_PERIPHERAL)
                }
                CortexMDmaTransferShape::PeripheralToMemory => {
                    caps.contains(CortexMDmaTransferCaps::PERIPHERAL_TO_MEMORY)
                }
                CortexMDmaTransferShape::ChannelChaining => {
                    caps.contains(CortexMDmaTransferCaps::CHANNEL_CHAINING)
                }
            }
        }
    }

    /// Typed RP2350 alias over the selected board's Cortex-M DMA request table.
    #[cfg(feature = "soc-rp2350")]
    pub type Rp2350DmaRequest = CortexMDmaRequest;

    /// Returns the consumer-facing policy for one raw DMA request descriptor.
    #[must_use]
    pub const fn policy_for_descriptor(
        descriptor: &CortexMDmaRequestDescriptor,
    ) -> CortexMDmaRequestPolicy {
        match descriptor.class {
            CortexMDmaRequestClass::PeripheralTx => CortexMDmaRequestPolicy {
                role: CortexMDmaConsumerRole::PeripheralTx,
                preferred_shape: Some(CortexMDmaTransferShape::MemoryToPeripheral),
            },
            CortexMDmaRequestClass::PeripheralRx => CortexMDmaRequestPolicy {
                role: CortexMDmaConsumerRole::PeripheralRx,
                preferred_shape: Some(CortexMDmaTransferShape::PeripheralToMemory),
            },
            CortexMDmaRequestClass::PeripheralPacer => CortexMDmaRequestPolicy {
                role: CortexMDmaConsumerRole::PeripheralPacer,
                preferred_shape: None,
            },
            CortexMDmaRequestClass::TimerPacer => CortexMDmaRequestPolicy {
                role: CortexMDmaConsumerRole::TimerPacer,
                preferred_shape: None,
            },
            CortexMDmaRequestClass::Force => CortexMDmaRequestPolicy {
                role: CortexMDmaConsumerRole::Force,
                preferred_shape: Some(CortexMDmaTransferShape::MemoryToMemory),
            },
        }
    }
}
