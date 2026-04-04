#![cfg(all(feature = "std", not(target_os = "none")))]

use fusion_pal::sys::dma::DmaBase;
use fusion_pal::sys::dma::DmaCaps;
use fusion_pal::sys::dma::DmaCatalog;
use fusion_pal::sys::dma::DmaImplementationKind;
use fusion_pal::sys::dma::system_dma;
use fusion_pal::sys::power::PowerBase;
use fusion_pal::sys::power::PowerCaps;
use fusion_pal::sys::power::PowerControl;
use fusion_pal::sys::power::PowerErrorKind;
use fusion_pal::sys::power::PowerImplementationKind;
use fusion_pal::sys::power::PowerModeDepth;
use fusion_pal::sys::power::system_power;

#[test]
fn dma_support_surface_is_exposed() {
    let dma = system_dma();
    let support = DmaBase::support(&dma);

    match support.implementation {
        DmaImplementationKind::Unsupported => {
            assert!(support.caps.is_empty());
            assert!(DmaCatalog::controllers(&dma).is_empty());
            assert!(DmaCatalog::requests(&dma).is_empty());
        }
        DmaImplementationKind::Native | DmaImplementationKind::Emulated => {
            assert!(!support.caps.is_empty());

            if support.caps.contains(DmaCaps::ENUMERATE_CONTROLLERS) {
                assert!(!DmaCatalog::controllers(&dma).is_empty());
            }

            if support.caps.contains(DmaCaps::ENUMERATE_REQUESTS) {
                assert!(!DmaCatalog::requests(&dma).is_empty());
            }
        }
    }
}

#[test]
fn power_support_surface_is_exposed() {
    let power = system_power();
    let support = PowerBase::support(&power);

    match support.implementation {
        PowerImplementationKind::Unsupported => {
            assert!(support.caps.is_empty());
            assert!(PowerControl::modes(&power).is_empty());
        }
        PowerImplementationKind::Native | PowerImplementationKind::Emulated => {
            assert!(support.caps.contains(PowerCaps::ENUMERATE));
            assert!(support.caps.contains(PowerCaps::ENTER));
        }
    }
}

#[test]
fn power_enter_mode_follows_backend_truth() {
    let power = system_power();
    let support = PowerBase::support(&power);

    if support.implementation == PowerImplementationKind::Unsupported {
        assert_eq!(
            PowerControl::enter_mode(&power, "anything")
                .expect_err("unsupported backend should reject power entry")
                .kind(),
            PowerErrorKind::Unsupported
        );
        return;
    }

    let modes = PowerControl::modes(&power);
    assert!(
        !modes.is_empty(),
        "supported backend should expose at least one power mode"
    );
    assert!(
        modes.iter().all(|mode| mode.depth != PowerModeDepth::Other),
        "current backends should classify their surfaced power modes concretely"
    );
}
