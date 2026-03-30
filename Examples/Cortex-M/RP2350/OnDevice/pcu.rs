use fusion_std::pcu::{
    Pcu,
    PcuErrorKind,
    PcuInvocationBindings,
    PcuInvocationHandle,
    PcuStreamPattern,
    PcuWordStreamBindings,
    system_pcu,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcuPioOnDeviceEvent {
    Starting { code: u16 },
    Passed { code: u16 },
    Failed { failure: PcuPioOnDeviceFailure },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PcuPioFailureStage {
    Planning = 0x1,
    Preparation = 0x2,
    Dispatch = 0x3,
    Completion = 0x4,
    Mismatch = 0x5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcuPioOnDeviceFailure {
    pub code: u16,
    pub stage: PcuPioFailureStage,
    pub index: usize,
    pub expected: u32,
    pub actual: u32,
    pub error_kind: Option<PcuErrorKind>,
}

impl PcuPioOnDeviceFailure {
    #[must_use]
    pub const fn display_code(self) -> u16 {
        ((self.stage as u16) << 12) | (self.code & 0x0fff)
    }
}

#[derive(Debug, Clone, Copy)]
struct PcuPioSmokeCase {
    code: u16,
    threads: u32,
    kernel_id: u32,
    entry_point: &'static str,
    patterns: &'static [PcuStreamPattern],
    input: &'static [u32],
    expected: &'static [u32],
}

const BIT_REVERSE_PATTERNS: [PcuStreamPattern; 1] = [PcuStreamPattern::BitReverse];
const SHIFT_LEFT_PATTERNS: [PcuStreamPattern; 1] = [PcuStreamPattern::ShiftLeft { bits: 8 }];
const SHIFT_RIGHT_PATTERNS: [PcuStreamPattern; 1] = [PcuStreamPattern::ShiftRight { bits: 4 }];
const EXTRACT_BITS_PATTERNS: [PcuStreamPattern; 1] = [PcuStreamPattern::ExtractBits {
    offset: 4,
    width: 12,
}];
const MASK_LOWER_PATTERNS: [PcuStreamPattern; 1] = [PcuStreamPattern::MaskLower { bits: 12 }];
const BYTE_SWAP32_PATTERNS: [PcuStreamPattern; 1] = [PcuStreamPattern::ByteSwap32];
const INCREMENT_PATTERNS: [PcuStreamPattern; 1] = [PcuStreamPattern::Increment];

const BIT_REVERSE_INPUT: [u32; 4] = [0x0000_00f0, 0x1234_5678, 0x8000_0001, 0xffff_0000];
const BIT_REVERSE_EXPECTED: [u32; 4] = [0x0f00_0000, 0x1e6a_2c48, 0x8000_0001, 0x0000_ffff];

const SHIFT_INPUT: [u32; 6] = [
    0x0000_00f0,
    0x1234_5678,
    0x8000_0001,
    0x00ff_00ff,
    0xf000_000f,
    0xabcd_ef01,
];
const SHIFT_LEFT_EXPECTED: [u32; 6] = [
    0x0000_f000,
    0x3456_7800,
    0x0000_0100,
    0xff00_ff00,
    0x0000_0f00,
    0xcdef_0100,
];
const SHIFT_RIGHT_EXPECTED: [u32; 6] = [
    0x0000_000f,
    0x0123_4567,
    0x0800_0000,
    0x000f_f00f,
    0x0f00_0000,
    0x0abc_def0,
];

const SLICE_INPUT: [u32; 4] = [0x1234_5678, 0xabcd_ef01, 0xf0f0_0f0f, 0x0000_ffff];
const EXTRACT_BITS_EXPECTED: [u32; 4] = [0x0567, 0x0ef0, 0x00f0, 0x0fff];
const MASK_LOWER_EXPECTED: [u32; 4] = [0x0678, 0x0f01, 0x0f0f, 0x0fff];
const BYTE_SWAP32_EXPECTED: [u32; 4] = [0x7856_3412, 0x01ef_cdab, 0x0f0f_f0f0, 0xffff_0000];
const INCREMENT_INPUT: [u32; 4] = [0, 41, 0xffff_fffe, 0xffff_ffff];
const INCREMENT_EXPECTED: [u32; 4] = [1, 42, 0xffff_ffff, 0];

const PCU_PIO_SMOKE_CASES: [PcuPioSmokeCase; 7] = [
    PcuPioSmokeCase {
        code: 0x001,
        threads: 4,
        kernel_id: 0x101,
        entry_point: "bit_reverse",
        patterns: &BIT_REVERSE_PATTERNS,
        input: &BIT_REVERSE_INPUT,
        expected: &BIT_REVERSE_EXPECTED,
    },
    PcuPioSmokeCase {
        code: 0x002,
        threads: 6,
        kernel_id: 0x102,
        entry_point: "shift_left_8",
        patterns: &SHIFT_LEFT_PATTERNS,
        input: &SHIFT_INPUT,
        expected: &SHIFT_LEFT_EXPECTED,
    },
    PcuPioSmokeCase {
        code: 0x003,
        threads: 6,
        kernel_id: 0x103,
        entry_point: "shift_right_4",
        patterns: &SHIFT_RIGHT_PATTERNS,
        input: &SHIFT_INPUT,
        expected: &SHIFT_RIGHT_EXPECTED,
    },
    PcuPioSmokeCase {
        code: 0x004,
        threads: 4,
        kernel_id: 0x104,
        entry_point: "extract_bits_4_12",
        patterns: &EXTRACT_BITS_PATTERNS,
        input: &SLICE_INPUT,
        expected: &EXTRACT_BITS_EXPECTED,
    },
    PcuPioSmokeCase {
        code: 0x005,
        threads: 4,
        kernel_id: 0x105,
        entry_point: "mask_lower_12",
        patterns: &MASK_LOWER_PATTERNS,
        input: &SLICE_INPUT,
        expected: &MASK_LOWER_EXPECTED,
    },
    PcuPioSmokeCase {
        code: 0x006,
        threads: 4,
        kernel_id: 0x106,
        entry_point: "byte_swap_32",
        patterns: &BYTE_SWAP32_PATTERNS,
        input: &SLICE_INPUT,
        expected: &BYTE_SWAP32_EXPECTED,
    },
    PcuPioSmokeCase {
        code: 0x007,
        threads: 4,
        kernel_id: 0x107,
        entry_point: "increment",
        patterns: &INCREMENT_PATTERNS,
        input: &INCREMENT_INPUT,
        expected: &INCREMENT_EXPECTED,
    },
];

#[must_use]
pub const fn suite_pass_display_code() -> u16 {
    0x600d
}

pub fn run_pcu_pio_smoke_suite(
    mut observe: impl FnMut(PcuPioOnDeviceEvent),
) -> Result<(), PcuPioOnDeviceFailure> {
    let system = system_pcu();
    for case in PCU_PIO_SMOKE_CASES {
        observe(PcuPioOnDeviceEvent::Starting { code: case.code });
        match run_case(&system, case) {
            Ok(()) => observe(PcuPioOnDeviceEvent::Passed { code: case.code }),
            Err(failure) => {
                observe(PcuPioOnDeviceEvent::Failed { failure });
                return Err(failure);
            }
        }
    }
    Ok(())
}

fn run_case(system: &Pcu, case: PcuPioSmokeCase) -> Result<(), PcuPioOnDeviceFailure> {
    let builder = system
        .pio_threads(case.threads)
        .map_err(|error| PcuPioOnDeviceFailure {
            code: case.code,
            stage: PcuPioFailureStage::Planning,
            index: 0,
            expected: 0,
            actual: 0,
            error_kind: Some(error.kind()),
        })?
        .words(case.kernel_id, case.entry_point)
        .with_patterns(case.patterns)
        .map_err(|error| PcuPioOnDeviceFailure {
            code: case.code,
            stage: PcuPioFailureStage::Planning,
            index: 0,
            expected: 0,
            actual: 0,
            error_kind: Some(error.kind()),
        })?;
    let kernel = builder.kernel();
    let descriptor = builder.descriptor(&kernel);

    let plan = system
        .plan(descriptor)
        .map_err(|error| PcuPioOnDeviceFailure {
            code: case.code,
            stage: PcuPioFailureStage::Planning,
            index: 0,
            expected: 0,
            actual: 0,
            error_kind: Some(error.kind()),
        })?;
    let prepared = system
        .prepare(plan)
        .map_err(|error| PcuPioOnDeviceFailure {
            code: case.code,
            stage: PcuPioFailureStage::Preparation,
            index: 0,
            expected: 0,
            actual: 0,
            error_kind: Some(error.kind()),
        })?;

    let mut output = [0u32; 8];
    let output_slice = &mut output[..case.expected.len()];
    let handle = prepared
        .dispatch(PcuInvocationBindings::StreamWords(PcuWordStreamBindings {
            input: case.input,
            output: output_slice,
        }))
        .map_err(|error| PcuPioOnDeviceFailure {
            code: case.code,
            stage: PcuPioFailureStage::Dispatch,
            index: 0,
            expected: 0,
            actual: 0,
            error_kind: Some(error.kind()),
        })?;

    handle.wait().map_err(|error| PcuPioOnDeviceFailure {
        code: case.code,
        stage: PcuPioFailureStage::Completion,
        index: 0,
        expected: 0,
        actual: 0,
        error_kind: Some(error.kind()),
    })?;

    for (index, (&actual, &expected)) in output_slice.iter().zip(case.expected.iter()).enumerate() {
        if actual != expected {
            return Err(PcuPioOnDeviceFailure {
                code: case.code,
                stage: PcuPioFailureStage::Mismatch,
                index,
                expected,
                actual,
                error_kind: None,
            });
        }
    }

    Ok(())
}
