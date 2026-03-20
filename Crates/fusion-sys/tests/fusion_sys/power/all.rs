use fusion_sys::power::{
    PowerCaps,
    PowerErrorKind,
    PowerImplementationKind,
    PowerModeDepth,
    PowerSystem,
};

#[test]
fn support_surface_is_exposed() {
    let power = PowerSystem::new();
    let support = power.support();

    match support.implementation {
        PowerImplementationKind::Unsupported => {
            assert!(support.caps.is_empty());
            assert!(power.modes().is_empty());
        }
        PowerImplementationKind::Native | PowerImplementationKind::Emulated => {
            assert!(support.caps.contains(PowerCaps::ENUMERATE));
            assert!(support.caps.contains(PowerCaps::ENTER));
        }
    }
}

#[test]
fn enter_mode_follows_backend_truth() {
    let power = PowerSystem::new();
    let support = power.support();

    if support.implementation == PowerImplementationKind::Unsupported {
        assert_eq!(
            power
                .enter_mode("anything")
                .expect_err("unsupported backend should reject power entry")
                .kind(),
            PowerErrorKind::Unsupported
        );
        return;
    }

    let modes = power.modes();
    assert!(
        !modes.is_empty(),
        "supported backend should expose at least one power mode"
    );
    assert!(
        modes.iter().all(|mode| mode.depth != PowerModeDepth::Other),
        "current backends should classify their surfaced power modes concretely"
    );
}
