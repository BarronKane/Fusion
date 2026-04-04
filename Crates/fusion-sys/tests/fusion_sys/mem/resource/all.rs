//! Capability-gated `fusion_sys::mem::resource` integration tests.
//!
//! These tests are written against the public contract rather than a specific backend. A
//! backend may satisfy a request or reject it, but it must do so consistently with the
//! support surface it advertises.

use fusion_pal::sys::mem::{
    IntegrityMode,
    MemPlacementCaps,
    Protect,
};
use fusion_sys::mem::resource::{
    CommitControlledResource,
    IntegrityConstraints,
    MemoryResource,
    OvercommitPolicy,
    PlacementPreference,
    RequiredPlacement,
    ResourceErrorKind,
    ResourceFeatureSupport,
    ResourceOpSet,
    ResourcePreferenceSet,
    ResourceRange,
    ResourceRequest,
    VirtualMemoryResource,
};

use super::support::page_len;

fn strict_no_overcommit_request(len: usize) -> ResourceRequest<'static> {
    let mut request = ResourceRequest::anonymous_private(len);
    request.contract.overcommit = OvercommitPolicy::Disallow;
    request
}

fn integrity_managed_request(len: usize) -> ResourceRequest<'static> {
    let mut request = ResourceRequest::anonymous_private(len);
    request.contract.integrity = Some(IntegrityConstraints {
        mode: IntegrityMode::Strict,
        tag: None,
    });
    request
}

#[test]
fn no_overcommit_profile_is_capability_gated() {
    let request = strict_no_overcommit_request(page_len(4));
    let support = VirtualMemoryResource::system_acquire_support();

    // If the backend claims stronger no-overcommit semantics, the request must succeed.
    // Otherwise the rejection should be explicit rather than silently degrading the contract.
    if support
        .features
        .contains(ResourceFeatureSupport::OVERCOMMIT_DISALLOW)
    {
        let _ = VirtualMemoryResource::create(&request)
            .expect("advertised no-overcommit support should admit the request");
    } else {
        let err = VirtualMemoryResource::create(&request)
            .expect_err("backend should reject unsupported no-overcommit request");
        assert_eq!(err.kind, ResourceErrorKind::UnsupportedRequest);
    }
}

#[test]
fn integrity_profile_is_capability_gated() {
    let request = integrity_managed_request(page_len(4));
    let support = VirtualMemoryResource::system_acquire_support();

    // Integrity-tagged acquisition is a hard contract knob, so unsupported backends must
    // reject it rather than pretending to honor it.
    if support.features.contains(ResourceFeatureSupport::INTEGRITY) {
        let _ = VirtualMemoryResource::create(&request)
            .expect("advertised integrity support should admit the request");
    } else {
        let err = VirtualMemoryResource::create(&request)
            .expect_err("backend should reject unsupported integrity request");
        assert_eq!(err.kind, ResourceErrorKind::UnsupportedRequest);
    }
}

#[test]
fn preferred_node_profile_degrades_to_unmet_preference_when_needed() {
    let mut request = ResourceRequest::anonymous_private(page_len(4));
    request.initial.placement = PlacementPreference::PreferredNode(0);
    request.preferences = ResourcePreferenceSet::PLACEMENT;
    let support = VirtualMemoryResource::system_acquire_support();

    // Preferred placement is allowed to degrade. If the backend lacks the capability but still
    // creates the resource, the unmet-preference bit must preserve that truth.
    match VirtualMemoryResource::create(&request) {
        Ok(resource) => {
            if !support
                .placements
                .contains(MemPlacementCaps::PREFERRED_NODE)
            {
                assert!(
                    resource
                        .resolved()
                        .unmet_preferences
                        .contains(ResourcePreferenceSet::PLACEMENT)
                );
            }
        }
        Err(err) => {
            assert_eq!(err.kind, ResourceErrorKind::UnsupportedRequest);
        }
    }
}

#[test]
fn required_node_profile_is_capability_gated() {
    let mut request = ResourceRequest::anonymous_private(page_len(2));
    request.required_placement = Some(RequiredPlacement::RequiredNode(0));
    let support = VirtualMemoryResource::system_acquire_support();

    // Required placement is not allowed to degrade. The backend either supports it or rejects
    // the request.
    if support.placements.contains(MemPlacementCaps::REQUIRED_NODE) {
        let _ = VirtualMemoryResource::create(&request)
            .expect("advertised required-node support should admit the request");
    } else {
        let err = VirtualMemoryResource::create(&request)
            .expect_err("backend should reject unsupported required-node request");
        assert_eq!(err.kind, ResourceErrorKind::UnsupportedRequest);
    }
}

#[test]
fn explicit_commit_control_is_capability_gated() {
    let resource =
        match VirtualMemoryResource::create(&ResourceRequest::anonymous_private(page_len(2))) {
            Ok(resource) => resource,
            Err(err) => {
                assert_eq!(err.kind, ResourceErrorKind::UnsupportedRequest);
                return;
            }
        };
    let whole = ResourceRange::whole(resource.len());

    // Commit/decommit control is reported per live instance. If those ops are present, they
    // must work on the whole range; otherwise the resource should reject them cleanly.
    if resource.ops().contains(ResourceOpSet::DECOMMIT) {
        unsafe { resource.decommit(whole) }.expect("advertised decommit should work");

        if resource.ops().contains(ResourceOpSet::COMMIT) {
            unsafe { resource.commit(whole, Protect::READ | Protect::WRITE) }
                .expect("advertised commit should work");
        }
    } else {
        let err = unsafe { resource.decommit(whole) }
            .expect_err("backend should reject unsupported decommit");
        assert_eq!(err.kind, ResourceErrorKind::UnsupportedOperation);

        let err = unsafe { resource.commit(whole, Protect::READ | Protect::WRITE) }
            .expect_err("backend should reject unsupported commit");
        assert_eq!(err.kind, ResourceErrorKind::UnsupportedOperation);
    }
}
