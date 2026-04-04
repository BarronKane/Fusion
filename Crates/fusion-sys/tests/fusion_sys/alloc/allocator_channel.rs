use core::pin::pin;

use fusion_sys::alloc::{
    AllocErrorKind,
    Allocator,
    AllocatorControlRequest,
};
use fusion_sys::channel::{
    ChannelReceive,
    ChannelSend,
};
use fusion_sys::fiber::{
    FiberMetadataMessage,
    FiberStack,
    FiberYield,
    ManagedFiber,
};
use fusion_sys::transport::{
    TransportAttachmentControl,
    TransportAttachmentRequest,
};

#[test]
fn allocator_channel_service_advertises_domains_and_serves_audits() {
    let allocator = Allocator::<4, 4>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let mut service: fusion_sys::alloc::AllocatorChannelService<'_, 4, 4, 64, 4, 4, 4> =
        fusion_sys::alloc::AllocatorChannelService::new(&allocator)
            .expect("allocator channel service should build");

    let metadata_consumer = service
        .metadata_channel()
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("metadata consumer should attach");
    let control_producer = service
        .control_channel()
        .attach_producer(TransportAttachmentRequest::same_courier())
        .expect("control producer should attach");
    let status_consumer = service
        .status_channel()
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("status consumer should attach");

    service.pump().expect("service should publish metadata");
    let metadata = service
        .metadata_channel()
        .try_receive(metadata_consumer)
        .expect("metadata receive should succeed")
        .expect("metadata message should exist");
    match metadata {
        fusion_sys::alloc::AllocatorDomainMetadataMessage::Advertised(info) => {
            assert_eq!(info.id, default_domain);
        }
        other => panic!("unexpected metadata message: {other:?}"),
    }

    service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainAudit {
                domain: default_domain,
            },
        )
        .expect("audit request should send");
    service.pump().expect("service should handle audit request");
    let status = service
        .status_channel()
        .try_receive(status_consumer)
        .expect("status receive should succeed")
        .expect("status message should exist");
    match status {
        fusion_sys::alloc::AllocatorControlStatusMessage::DomainAudit { domain, audit } => {
            assert_eq!(domain, default_domain);
            assert_eq!(audit.info.id, default_domain);
            assert!(audit.pool_stats.is_some());
        }
        other => panic!("unexpected audit status: {other:?}"),
    }

    service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainPoolMembers {
                domain: default_domain,
            },
        )
        .expect("member stream request should send");
    service
        .pump()
        .expect("service should stream pool-member snapshots");
    let first_member = service
        .status_channel()
        .try_receive(status_consumer)
        .expect("member receive should succeed")
        .expect("first member message should exist");
    match first_member {
        fusion_sys::alloc::AllocatorControlStatusMessage::DomainPoolMember { domain, member } => {
            assert_eq!(domain, default_domain);
            assert_eq!(member.id.0, 0);
            assert_ne!(member.resource.range().base.get(), 0);
            assert!(member.resource.range().len >= member.usable_range.len);
        }
        other => panic!("unexpected member status: {other:?}"),
    }
    let member_done = service
        .status_channel()
        .try_receive(status_consumer)
        .expect("member completion receive should succeed")
        .expect("member completion should exist");
    match member_done {
        fusion_sys::alloc::AllocatorControlStatusMessage::DomainPoolMembersComplete { domain } => {
            assert_eq!(domain, default_domain);
        }
        other => panic!("unexpected member completion: {other:?}"),
    }

    service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainPoolExtents {
                domain: default_domain,
            },
        )
        .expect("extent stream request should send");
    service
        .pump()
        .expect("service should stream pool-extent snapshots");
    let first_extent = service
        .status_channel()
        .try_receive(status_consumer)
        .expect("extent receive should succeed")
        .expect("first extent message should exist");
    match first_extent {
        fusion_sys::alloc::AllocatorControlStatusMessage::DomainPoolExtent { domain, extent } => {
            assert_eq!(domain, default_domain);
            assert_eq!(extent.member.0, 0);
            assert!(matches!(
                extent.disposition,
                fusion_sys::alloc::MemoryPoolExtentDisposition::Free
            ));
            assert!(extent.range.len > 0);
        }
        other => panic!("unexpected extent status: {other:?}"),
    }
    let extent_done = service
        .status_channel()
        .try_receive(status_consumer)
        .expect("extent completion receive should succeed")
        .expect("extent completion should exist");
    match extent_done {
        fusion_sys::alloc::AllocatorControlStatusMessage::DomainPoolExtentsComplete { domain } => {
            assert_eq!(domain, default_domain);
        }
        other => panic!("unexpected extent completion: {other:?}"),
    }

    service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainAudit {
                domain: fusion_sys::alloc::AllocatorDomainId(u16::MAX),
            },
        )
        .expect("invalid audit request should send");
    service
        .pump()
        .expect("service should reject invalid audit request honestly");
    let rejected = service
        .status_channel()
        .try_receive(status_consumer)
        .expect("rejected status receive should succeed")
        .expect("rejected status message should exist");
    match rejected {
        fusion_sys::alloc::AllocatorControlStatusMessage::Rejected { domain, reason } => {
            assert_eq!(domain, Some(fusion_sys::alloc::AllocatorDomainId(u16::MAX)));
            assert_eq!(reason, AllocErrorKind::InvalidDomain);
        }
        other => panic!("unexpected rejected status: {other:?}"),
    }
}

#[test]
fn allocator_channel_service_can_run_on_managed_fiber() {
    let allocator = Allocator::<4, 4>::system_default().expect("allocator should build");
    let default_domain = allocator
        .default_domain()
        .expect("default domain should exist");
    let service: fusion_sys::alloc::AllocatorChannelService<'_, 4, 4, 64, 4, 4, 4> =
        fusion_sys::alloc::AllocatorChannelService::new(&allocator)
            .expect("allocator channel service should build");
    let mut service = pin!(service);
    let mut stack_words = vec![0_u128; 2048].into_boxed_slice();
    let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should be valid");
    let mut fiber = ManagedFiber::<_, 8, 8>::new_with_publication(service.as_mut(), stack)
        .expect("managed allocator service fiber should build");

    let fiber_consumer = fiber
        .metadata_channel()
        .expect("allocator service fiber should expose explicit publication")
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("fiber metadata consumer should attach");
    let metadata_consumer = fiber
        .state()
        .metadata_channel()
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("metadata consumer should attach");
    let control_producer = fiber
        .state()
        .control_channel()
        .attach_producer(TransportAttachmentRequest::same_courier())
        .expect("control producer should attach");
    let status_consumer = fiber
        .state()
        .status_channel()
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("status consumer should attach");

    assert_eq!(
        fiber
            .metadata_channel()
            .expect("allocator service fiber should expose explicit publication")
            .try_receive(fiber_consumer)
            .expect("fiber metadata receive should succeed"),
        Some(FiberMetadataMessage::Created { fiber: fiber.id() })
    );

    assert!(matches!(
        fiber.resume().expect("service fiber should yield"),
        FiberYield::Yielded
    ));

    assert_eq!(
        fiber
            .metadata_channel()
            .expect("allocator service fiber should expose explicit publication")
            .try_receive(fiber_consumer)
            .expect("fiber metadata receive should succeed"),
        Some(FiberMetadataMessage::Started { fiber: fiber.id() })
    );

    let metadata = fiber
        .state()
        .metadata_channel()
        .try_receive(metadata_consumer)
        .expect("metadata receive should succeed")
        .expect("metadata message should exist");
    match metadata {
        fusion_sys::alloc::AllocatorDomainMetadataMessage::Advertised(info) => {
            assert_eq!(info.id, default_domain);
        }
        other => panic!("unexpected metadata message: {other:?}"),
    }

    fiber
        .state()
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainAudit {
                domain: default_domain,
            },
        )
        .expect("audit request should send");

    assert!(matches!(
        fiber.resume().expect("service fiber should yield"),
        FiberYield::Yielded
    ));

    let status = fiber
        .state()
        .status_channel()
        .try_receive(status_consumer)
        .expect("status receive should succeed")
        .expect("status should exist");
    match status {
        fusion_sys::alloc::AllocatorControlStatusMessage::DomainAudit { domain, audit } => {
            assert_eq!(domain, default_domain);
            assert_eq!(audit.info.id, default_domain);
        }
        other => panic!("unexpected audit status: {other:?}"),
    }
}
