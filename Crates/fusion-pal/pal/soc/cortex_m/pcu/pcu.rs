//! Cortex-M coprocessor backend.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::contract::drivers::pcu::{
    PcuBase,
    PcuCaps,
    PcuControl,
    PcuError,
    PcuExecutorClaim,
    PcuExecutorClass,
    PcuExecutorDescriptor,
    PcuExecutorId,
    PcuExecutorOrigin,
    PcuSupport,
};

use crate::pal::soc::cortex_m::hal::soc::pio::{PioBase, system_pio};

const CORTEX_M_CPU_EXECUTOR_ID: PcuExecutorId = PcuExecutorId(0);
const MAX_CORTEX_M_PIO_EXECUTORS: usize = 8;

const fn cpu_executor() -> PcuExecutorDescriptor {
    PcuExecutorDescriptor {
        id: CORTEX_M_CPU_EXECUTOR_ID,
        name: "cortex-m-cpu",
        class: PcuExecutorClass::Cpu,
        origin: PcuExecutorOrigin::Synthetic,
    }
}

const fn pio_executor(id: u8, name: &'static str) -> PcuExecutorDescriptor {
    PcuExecutorDescriptor {
        id: PcuExecutorId(id),
        name,
        class: PcuExecutorClass::Io,
        origin: PcuExecutorOrigin::TopologyBound,
    }
}

static CORTEX_M_EXECUTORS_0: [PcuExecutorDescriptor; 1] = [cpu_executor()];
static CORTEX_M_EXECUTORS_1: [PcuExecutorDescriptor; 2] =
    [cpu_executor(), pio_executor(1, "cortex-m-pio0")];
static CORTEX_M_EXECUTORS_2: [PcuExecutorDescriptor; 3] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
];
static CORTEX_M_EXECUTORS_3: [PcuExecutorDescriptor; 4] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
];
static CORTEX_M_EXECUTORS_4: [PcuExecutorDescriptor; 5] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
];
static CORTEX_M_EXECUTORS_5: [PcuExecutorDescriptor; 6] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
];
static CORTEX_M_EXECUTORS_6: [PcuExecutorDescriptor; 7] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
];
static CORTEX_M_EXECUTORS_7: [PcuExecutorDescriptor; 8] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
    pio_executor(7, "cortex-m-pio6"),
];
static CORTEX_M_EXECUTORS_8: [PcuExecutorDescriptor; 9] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
    pio_executor(7, "cortex-m-pio6"),
    pio_executor(8, "cortex-m-pio7"),
];
static CORTEX_M_CPU_EXECUTOR_CLAIMED: AtomicBool = AtomicBool::new(false);
static CORTEX_M_PIO_EXECUTOR_CLAIMED: [AtomicBool; MAX_CORTEX_M_PIO_EXECUTORS] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

fn pio_executor_count() -> usize {
    core::cmp::min(system_pio().engines().len(), MAX_CORTEX_M_PIO_EXECUTORS)
}

fn cortex_m_executors() -> &'static [PcuExecutorDescriptor] {
    match pio_executor_count() {
        0 => &CORTEX_M_EXECUTORS_0,
        1 => &CORTEX_M_EXECUTORS_1,
        2 => &CORTEX_M_EXECUTORS_2,
        3 => &CORTEX_M_EXECUTORS_3,
        4 => &CORTEX_M_EXECUTORS_4,
        5 => &CORTEX_M_EXECUTORS_5,
        6 => &CORTEX_M_EXECUTORS_6,
        7 => &CORTEX_M_EXECUTORS_7,
        _ => &CORTEX_M_EXECUTORS_8,
    }
}

/// Cortex-M coprocessor provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMPcu;

/// Selected Cortex-M programmable-IO provider type.
pub type PlatformPcu = CortexMPcu;

/// Returns the selected Cortex-M coprocessor provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl CortexMPcu {
    /// Creates a new Cortex-M coprocessor provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBase for CortexMPcu {
    fn support(&self) -> PcuSupport {
        let support = system_pio().support();
        PcuSupport {
            caps: PcuCaps::ENUMERATE_EXECUTORS
                | PcuCaps::CLAIM_EXECUTOR
                | PcuCaps::DISPATCH
                | PcuCaps::COMPLETION_STATUS
                | PcuCaps::EXTERNAL_RESOURCES,
            // Overall generic-PCU support is still native on CPU-only Cortex-M targets even when
            // no topology-bound PIO executor is surfaced.
            implementation: if support.engine_count == 0 {
                crate::contract::drivers::pcu::PcuImplementationKind::Native
            } else {
                support.implementation
            },
            executor_count: cortex_m_executors().len() as u8,
        }
    }

    fn executors(&self) -> &'static [PcuExecutorDescriptor] {
        cortex_m_executors()
    }
}

impl PcuControl for CortexMPcu {
    fn claim_executor(&self, executor: PcuExecutorId) -> Result<PcuExecutorClaim, PcuError> {
        match executor {
            CORTEX_M_CPU_EXECUTOR_ID => {
                CORTEX_M_CPU_EXECUTOR_CLAIMED
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| PcuError::busy())?;
            }
            PcuExecutorId(index) => {
                if index == 0 || usize::from(index) > pio_executor_count() {
                    return Err(PcuError::invalid());
                }
                CORTEX_M_PIO_EXECUTOR_CLAIMED[usize::from(index - 1)]
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| PcuError::busy())?;
            }
        }
        Ok(PcuExecutorClaim { executor })
    }

    fn release_executor(&self, claim: PcuExecutorClaim) -> Result<(), PcuError> {
        match claim.executor() {
            CORTEX_M_CPU_EXECUTOR_ID => {
                if !CORTEX_M_CPU_EXECUTOR_CLAIMED.swap(false, Ordering::AcqRel) {
                    return Err(PcuError::state_conflict());
                }
            }
            PcuExecutorId(index) => {
                if index == 0 || usize::from(index) > pio_executor_count() {
                    return Err(PcuError::invalid());
                }
                if !CORTEX_M_PIO_EXECUTOR_CLAIMED[usize::from(index - 1)]
                    .swap(false, Ordering::AcqRel)
                {
                    return Err(PcuError::state_conflict());
                }
            }
        }
        Ok(())
    }
}
