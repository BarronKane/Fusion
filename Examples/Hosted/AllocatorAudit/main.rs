use fusion_sys::alloc::{
    AllocModeSet,
    Allocator,
    AllocatorChannelService,
    AllocatorChannelServiceErrorKind,
    AllocatorControlRequest,
    AllocatorControlStatusMessage,
    AllocatorControlStatusProtocol,
    AllocatorDomainAudit,
    AllocatorDomainId,
    AllocatorDomainKind,
    AllocatorDomainMetadataMessage,
    AllocatorDomainMetadataProtocol,
    MemoryPoolExtentDisposition,
    MemoryPoolExtentInfo,
    MemoryPoolMemberId,
    MemoryPoolMemberInfo,
};
use fusion_sys::channel::{
    ChannelReceive,
    ChannelSend,
};
use fusion_sys::fiber::{
    ContextCaps,
    FiberMetadataMessage,
    FiberReturn,
    FiberRunnable,
    FiberStack,
    FiberSystem,
    FiberYield,
    ManagedFiber,
    yield_now,
};
use fusion_sys::insight::{
    InsightCaptureMode,
    InsightChannelClass,
    LocalInsightChannel,
};
use fusion_sys::mem::resource::AllocatorLayoutRealization;
use fusion_sys::mem::resource::{
    MemoryResourceHandle,
    ResourceRequest,
    VirtualMemoryResource,
};
use fusion_sys::protocol::{
    Protocol,
    ProtocolBootstrapKind,
    ProtocolCaps,
    ProtocolDebugView,
    ProtocolDescriptor,
    ProtocolId,
    ProtocolImplementationKind,
    ProtocolTransportRequirements,
    ProtocolVersion,
};
use fusion_sys::transport::{
    TransportAttachmentControl,
    TransportAttachmentRequest,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum DemoTimelineEvent {
    FiberStarted,
    MetadataPump,
    AuditPump,
    PoolStatsPump,
    PoolMembersPump,
    PoolExtentsPump,
    RepublishPump,
    FiberCompleted,
}

struct DemoTimelineProtocol;

impl Protocol for DemoTimelineProtocol {
    type Message = DemoTimelineEvent;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_414c_4c4f_435f_544c_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::DEBUG_VIEW,
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

struct DemoAuditInsightProtocol;

impl Protocol for DemoAuditInsightProtocol {
    type Message = AllocatorDomainAudit;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_414c_4c4f_435f_4155_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::DEBUG_VIEW,
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

type DemoAllocator = Allocator<2, 2, 64>;
type DemoService = AllocatorChannelService<'static, 2, 2, 64, 8, 8, 8>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DemoPhase {
    Metadata,
    Audit,
    PoolStats,
    PoolMembers,
    PoolExtents,
    Republish,
    Complete,
}

struct DemoState {
    allocator: &'static DemoAllocator,
    service: DemoService,
    default_domain: AllocatorDomainId,
    timeline: LocalInsightChannel<DemoTimelineProtocol, 16>,
    timeline_producer: usize,
    audit: LocalInsightChannel<DemoAuditInsightProtocol, 16>,
    audit_producer: usize,
    phase: DemoPhase,
}

impl DemoState {
    fn pump_and_emit(&mut self, event: DemoTimelineEvent) -> Result<(), String> {
        self.service.pump().map_err(|error| {
            format!(
                "allocator service pump failed: {}",
                allocator_service_error_kind_name(error.kind())
            )
        })?;
        self.timeline
            .try_send_if_observed(self.timeline_producer, || event)
            .map_err(|error| format!("timeline insight send failed: {error}"))?;

        if self.audit.is_observed() {
            let audit = self
                .allocator
                .domain_audit(self.default_domain)
                .map_err(|error| format!("domain audit failed: {error}"))?;
            self.audit
                .try_send(self.audit_producer, audit)
                .map_err(|error| format!("audit insight send failed: {error}"))?;
        }
        Ok(())
    }
}

impl FiberRunnable for DemoState {
    fn run(mut self: core::pin::Pin<&mut Self>) -> FiberReturn {
        let state = self.as_mut().get_mut();

        if let Err(error) = state
            .timeline
            .try_send_if_observed(state.timeline_producer, || DemoTimelineEvent::FiberStarted)
        {
            eprintln!("allocator_audit: failed to emit startup insight: {error}");
            return FiberReturn::new(1);
        }

        loop {
            let pump_result = match state.phase {
                DemoPhase::Metadata => {
                    let result = state.pump_and_emit(DemoTimelineEvent::MetadataPump);
                    state.phase = DemoPhase::Audit;
                    result
                }
                DemoPhase::Audit => {
                    let result = state.pump_and_emit(DemoTimelineEvent::AuditPump);
                    state.phase = DemoPhase::PoolStats;
                    result
                }
                DemoPhase::PoolStats => {
                    let result = state.pump_and_emit(DemoTimelineEvent::PoolStatsPump);
                    state.phase = DemoPhase::PoolMembers;
                    result
                }
                DemoPhase::PoolMembers => {
                    let result = state.pump_and_emit(DemoTimelineEvent::PoolMembersPump);
                    state.phase = DemoPhase::PoolExtents;
                    result
                }
                DemoPhase::PoolExtents => {
                    let result = state.pump_and_emit(DemoTimelineEvent::PoolExtentsPump);
                    state.phase = DemoPhase::Republish;
                    result
                }
                DemoPhase::Republish => {
                    let result = state.pump_and_emit(DemoTimelineEvent::RepublishPump);
                    state.phase = DemoPhase::Complete;
                    result
                }
                DemoPhase::Complete => {
                    let result = state.pump_and_emit(DemoTimelineEvent::FiberCompleted);
                    if let Err(error) = result {
                        eprintln!("allocator_audit: {error}");
                        return FiberReturn::new(1);
                    }
                    return FiberReturn::new(0);
                }
            };

            if let Err(error) = pump_result {
                eprintln!("allocator_audit: {error}");
                return FiberReturn::new(1);
            }

            if let Err(error) = yield_now() {
                eprintln!("allocator_audit: fiber yield failed: {error}");
                return FiberReturn::new(1);
            }
        }
    }
}

fn print_rule(title: &str) {
    println!("\n== {title} ==");
}

fn allocator_domain_id_value(id: AllocatorDomainId) -> u16 {
    id.0
}

fn print_allocator_domain_slot(domain: Option<AllocatorDomainId>) {
    match domain {
        Some(domain) => print!("{}", allocator_domain_id_value(domain)),
        None => print!("global"),
    }
}

fn allocator_domain_kind_name(kind: AllocatorDomainKind) -> &'static str {
    match kind {
        AllocatorDomainKind::Default => "default",
        AllocatorDomainKind::Explicit => "explicit",
    }
}

fn allocator_layout_realization_name(realization: AllocatorLayoutRealization) -> &'static str {
    match realization {
        AllocatorLayoutRealization::LazyVirtual => "lazy-virtual",
        AllocatorLayoutRealization::EagerPhysical => "eager-physical",
    }
}

fn allocator_service_error_kind_name(kind: AllocatorChannelServiceErrorKind) -> String {
    match kind {
        AllocatorChannelServiceErrorKind::Alloc(kind) => format!("alloc:{kind}"),
        AllocatorChannelServiceErrorKind::Channel(kind) => format!("channel:{kind}"),
        AllocatorChannelServiceErrorKind::Transport(kind) => format!("transport:{kind}"),
    }
}

fn insight_availability_name(
    availability: fusion_sys::insight::InsightAvailabilityKind,
) -> &'static str {
    match availability {
        fusion_sys::insight::InsightAvailabilityKind::Available => "available",
        fusion_sys::insight::InsightAvailabilityKind::DisabledByFeature => "disabled-by-feature",
    }
}

fn insight_channel_class_name(class: InsightChannelClass) -> &'static str {
    match class {
        InsightChannelClass::Timeline => "timeline",
        InsightChannelClass::State => "state",
        InsightChannelClass::Snapshot => "snapshot",
        InsightChannelClass::Control => "control",
    }
}

fn insight_capture_mode_name(capture: InsightCaptureMode) -> &'static str {
    match capture {
        InsightCaptureMode::Lossy => "lossy",
        InsightCaptureMode::Exact => "exact",
    }
}

fn timeline_name(event: DemoTimelineEvent) -> &'static str {
    match event {
        DemoTimelineEvent::FiberStarted => "fiber-started",
        DemoTimelineEvent::MetadataPump => "metadata-pump",
        DemoTimelineEvent::AuditPump => "audit-pump",
        DemoTimelineEvent::PoolStatsPump => "pool-stats-pump",
        DemoTimelineEvent::PoolMembersPump => "pool-members-pump",
        DemoTimelineEvent::PoolExtentsPump => "pool-extents-pump",
        DemoTimelineEvent::RepublishPump => "republish-pump",
        DemoTimelineEvent::FiberCompleted => "fiber-completed",
    }
}

fn print_modes(modes: AllocModeSet) {
    println!(
        "  modes: slab={} arena={} heap={} global={}",
        modes.contains(AllocModeSet::SLAB),
        modes.contains(AllocModeSet::ARENA),
        modes.contains(AllocModeSet::HEAP),
        modes.contains(AllocModeSet::GLOBAL_ALLOC)
    );
}

fn print_domain_audit(label: &str, audit: &AllocatorDomainAudit) {
    println!("{label}:");
    println!(
        "  domain: {} ({})",
        allocator_domain_id_value(audit.info.id),
        allocator_domain_kind_name(audit.info.kind)
    );
    print_modes(audit.info.policy.modes);
    println!("  safety bits: 0x{:x}", audit.info.policy.safety.bits());
    println!("  resources: {}", audit.info.resource_count);
    println!(
        "  memory domains bits: 0x{:x}",
        audit.info.memory_domains.bits()
    );
    println!("  attrs bits: 0x{:x}", audit.info.attrs.bits());
    println!("  hazards bits: 0x{:x}", audit.info.hazards.bits());
    match audit.primary_layout_policy {
        Some(policy) => {
            println!(
                "  layout: metadata_granule={} min_extent_align={} arena_align={} slab_align={} realization={}",
                policy.metadata_granule,
                policy.min_extent_align,
                policy.default_arena_align,
                policy.default_slab_align,
                allocator_layout_realization_name(policy.realization)
            );
        }
        None => println!("  layout: <none>"),
    }
    match audit.pool_stats {
        Some(stats) => print_pool_stats("  pool", stats),
        None => println!("  pool: <none>"),
    }
}

fn print_pool_stats(label: &str, stats: fusion_sys::alloc::MemoryPoolStats) {
    println!(
        "{label}: total={} free={} leased={} largest_free={}",
        stats.total_bytes, stats.free_bytes, stats.leased_bytes, stats.largest_free_extent
    );
    println!(
        "  extents: free={} leased={} slots={}/{}",
        stats.free_extent_count,
        stats.leased_extent_count,
        stats.extent_slots_used,
        stats.extent_slot_capacity
    );
}

fn print_metadata_message(message: AllocatorDomainMetadataMessage) {
    match message {
        AllocatorDomainMetadataMessage::Advertised(info) => {
            println!(
                "metadata: advertised {}",
                allocator_domain_id_value(info.id)
            );
            println!(
                "  kind: {} resources: {}",
                allocator_domain_kind_name(info.kind),
                info.resource_count
            );
            print_modes(info.policy.modes);
            println!("  attrs bits: 0x{:x}", info.attrs.bits());
            println!("  hazards bits: 0x{:x}", info.hazards.bits());
        }
        AllocatorDomainMetadataMessage::Withdrawn(domain) => {
            println!("metadata: withdrawn {}", allocator_domain_id_value(domain));
        }
    }
}

fn print_status_message(
    message: AllocatorControlStatusMessage,
    member_cache: &mut Vec<(MemoryPoolMemberId, MemoryPoolMemberInfo)>,
) {
    match message {
        AllocatorControlStatusMessage::DomainAudit { domain, audit } => {
            println!("status: domain-audit {}", allocator_domain_id_value(domain));
            print_domain_audit("  audit", &audit);
        }
        AllocatorControlStatusMessage::DomainPoolStats { domain, stats } => {
            println!(
                "status: domain-pool-stats {}",
                allocator_domain_id_value(domain)
            );
            match stats {
                Some(stats) => print_pool_stats("  pool", stats),
                None => println!("  pool: <none>"),
            }
        }
        AllocatorControlStatusMessage::DomainPoolMember { domain, member } => {
            println!(
                "status: domain-pool-member {}",
                allocator_domain_id_value(domain)
            );
            print_member_info(member);
            record_member(member_cache, member);
        }
        AllocatorControlStatusMessage::DomainPoolMembersComplete { domain } => {
            println!(
                "status: domain-pool-members-complete {}",
                allocator_domain_id_value(domain)
            );
        }
        AllocatorControlStatusMessage::DomainPoolExtent { domain, extent } => {
            println!(
                "status: domain-pool-extent {}",
                allocator_domain_id_value(domain)
            );
            print_extent_info(member_cache, extent);
        }
        AllocatorControlStatusMessage::DomainPoolExtentsComplete { domain } => {
            println!(
                "status: domain-pool-extents-complete {}",
                allocator_domain_id_value(domain)
            );
        }
        AllocatorControlStatusMessage::MetadataRepublishScheduled => {
            println!("status: metadata-republish-scheduled");
        }
        AllocatorControlStatusMessage::Rejected { domain, reason } => {
            print!("status: rejected domain=");
            print_allocator_domain_slot(domain);
            println!(" reason={reason}");
        }
    }
}

fn print_timeline_message(message: DemoTimelineEvent) {
    println!("insight.timeline: {}", timeline_name(message));
}

fn pool_member_id_value(id: MemoryPoolMemberId) -> u32 {
    id.0
}

fn memory_pool_extent_disposition_name(disposition: MemoryPoolExtentDisposition) -> &'static str {
    match disposition {
        MemoryPoolExtentDisposition::Free => "free",
        MemoryPoolExtentDisposition::Leased(_) => "leased",
    }
}

fn print_absolute_range(label: &str, base: usize, len: usize) {
    println!(
        "{label}: 0x{base:016x}..0x{:016x} ({} bytes)",
        base.saturating_add(len),
        len
    );
}

fn print_relative_range(label: &str, offset: usize, len: usize) {
    println!(
        "{label}: +0x{offset:08x}..+0x{:08x} ({} bytes)",
        offset.saturating_add(len),
        len
    );
}

fn print_member_info(member: MemoryPoolMemberInfo) {
    let resource = member.resource.range();
    println!("  member: {}", pool_member_id_value(member.id));
    print_absolute_range("    resource", resource.base.get(), resource.len);
    print_relative_range(
        "    usable",
        member.usable_range.offset,
        member.usable_range.len,
    );
    print_absolute_range(
        "    usable.abs",
        resource
            .base
            .get()
            .saturating_add(member.usable_range.offset),
        member.usable_range.len,
    );
    println!(
        "    free={} leased={} largest_free={}",
        member.free_bytes, member.leased_bytes, member.largest_free_extent
    );
}

fn extent_absolute_base(
    member_cache: &[(MemoryPoolMemberId, MemoryPoolMemberInfo)],
    extent: MemoryPoolExtentInfo,
) -> Option<usize> {
    member_cache
        .iter()
        .find_map(|(id, member)| {
            (*id == extent.member).then_some(member.resource.range().base.get())
        })
        .map(|base| base.saturating_add(extent.range.offset))
}

fn print_extent_info(
    member_cache: &[(MemoryPoolMemberId, MemoryPoolMemberInfo)],
    extent: MemoryPoolExtentInfo,
) {
    println!(
        "  extent: member={} state={}",
        pool_member_id_value(extent.member),
        memory_pool_extent_disposition_name(extent.disposition)
    );
    if let MemoryPoolExtentDisposition::Leased(id) = extent.disposition {
        println!("    lease: {}", id.0);
    }
    print_relative_range("    relative", extent.range.offset, extent.range.len);
    match extent_absolute_base(member_cache, extent) {
        Some(base) => print_absolute_range("    absolute", base, extent.range.len),
        None => println!("    absolute: <member-base-unavailable>"),
    }
}

fn record_member(
    cache: &mut Vec<(MemoryPoolMemberId, MemoryPoolMemberInfo)>,
    member: MemoryPoolMemberInfo,
) {
    if let Some(slot) = cache.iter_mut().find(|(id, _)| *id == member.id) {
        slot.1 = member;
    } else {
        cache.push((member.id, member));
    }
}

fn print_fiber_yield(label: &str, outcome: FiberYield) {
    match outcome {
        FiberYield::Yielded => println!("{label}: yielded"),
        FiberYield::Completed(FiberReturn { code }) => {
            println!("{label}: completed({code})");
        }
    }
}

fn print_fiber_metadata_message(message: FiberMetadataMessage) {
    match message {
        FiberMetadataMessage::Created { fiber } => {
            println!("fiber.metadata: created({})", fiber.get());
        }
        FiberMetadataMessage::Started { fiber } => {
            println!("fiber.metadata: started({})", fiber.get());
        }
        FiberMetadataMessage::Completed { fiber, result } => {
            println!(
                "fiber.metadata: completed({}, code={})",
                fiber.get(),
                result.code
            );
        }
        FiberMetadataMessage::Faulted { fiber, reason } => {
            println!("fiber.metadata: faulted({}, reason={reason})", fiber.get());
        }
        FiberMetadataMessage::ClaimAwarenessChanged {
            fiber,
            awareness,
            claim_context,
        } => {
            println!(
                "fiber.metadata: claim-awareness({}, mode={awareness:?}, context={claim_context:?})",
                fiber.get()
            );
        }
        FiberMetadataMessage::Abandoned { fiber, lifecycle } => {
            println!(
                "fiber.metadata: abandoned({}, state={})",
                fiber.get(),
                fiber_state_name(lifecycle)
            );
        }
    }
}

fn fiber_state_name(state: fusion_sys::fiber::FiberState) -> &'static str {
    match state {
        fusion_sys::fiber::FiberState::Created => "created",
        fusion_sys::fiber::FiberState::Running => "running",
        fusion_sys::fiber::FiberState::Suspended => "suspended",
        fusion_sys::fiber::FiberState::Completed => "completed",
    }
}

fn drain_fiber_metadata(channel: &fusion_sys::fiber::FiberMetadataChannel<16>, consumer: usize) {
    loop {
        match channel.try_receive(consumer) {
            Ok(Some(message)) => print_fiber_metadata_message(message),
            Ok(None) => break,
            Err(error) => {
                eprintln!("allocator_audit: fiber metadata receive failed: {error}");
                break;
            }
        }
    }
}

fn drain_metadata(
    channel: &fusion_sys::channel::LocalChannel<AllocatorDomainMetadataProtocol, 8>,
    consumer: usize,
) {
    loop {
        match channel.try_receive(consumer) {
            Ok(Some(message)) => print_metadata_message(message),
            Ok(None) => break,
            Err(error) => {
                eprintln!("allocator_audit: metadata receive failed: {error}");
                break;
            }
        }
    }
}

fn drain_status(
    channel: &fusion_sys::channel::LocalChannel<AllocatorControlStatusProtocol, 8>,
    consumer: usize,
    member_cache: &mut Vec<(MemoryPoolMemberId, MemoryPoolMemberInfo)>,
) {
    loop {
        match channel.try_receive(consumer) {
            Ok(Some(message)) => print_status_message(message, member_cache),
            Ok(None) => break,
            Err(error) => {
                eprintln!("allocator_audit: status receive failed: {error}");
                break;
            }
        }
    }
}

fn drain_timeline(channel: &LocalInsightChannel<DemoTimelineProtocol, 16>, consumer: usize) {
    loop {
        match channel.try_receive(consumer) {
            Ok(Some(message)) => print_timeline_message(message),
            Ok(None) => break,
            Err(error) => {
                eprintln!("allocator_audit: timeline receive failed: {error}");
                break;
            }
        }
    }
}

fn drain_audit(channel: &LocalInsightChannel<DemoAuditInsightProtocol, 16>, consumer: usize) {
    loop {
        match channel.try_receive(consumer) {
            Ok(Some(message)) => print_domain_audit("insight.state", &message),
            Ok(None) => break,
            Err(error) => {
                eprintln!("allocator_audit: audit receive failed: {error}");
                break;
            }
        }
    }
}

fn main() {
    let support = FiberSystem::new().support();
    if !support.context.caps.contains(ContextCaps::MAKE) {
        eprintln!("allocator_audit: low-level fibers are unsupported on this hosted backend");
        return;
    }

    // Deliberately overprovision one explicit hosted backing so the demo can keep live slab and
    // arena extents around while still showing the allocator's remaining free-space map.
    let mut resource_request = ResourceRequest::anonymous_private(64 * 1024);
    resource_request.name = Some("allocator-audit-example");
    let resource = VirtualMemoryResource::create(&resource_request)
        .expect("hosted allocator backing resource should build");
    let allocator = Box::leak(Box::new(
        DemoAllocator::from_resource_with_policy(
            MemoryResourceHandle::from(resource),
            fusion_sys::alloc::AllocPolicy::critical_safe(),
        )
        .expect("hosted allocator should build"),
    ));
    let default_domain = allocator
        .default_domain()
        .expect("default allocator domain should exist");
    let _demo_slab = allocator
        .slab::<64, 8>(default_domain)
        .expect("demo slab should reserve one live extent");
    let _demo_arena = allocator
        .arena(default_domain, 2048)
        .expect("demo arena should reserve one live extent");

    let timeline = LocalInsightChannel::<DemoTimelineProtocol, 16>::new(
        InsightChannelClass::Timeline,
        InsightCaptureMode::Lossy,
    )
    .expect("allocator_audit requires debug-insights to be enabled");
    let timeline_producer = timeline
        .attach_producer(TransportAttachmentRequest::same_courier())
        .expect("timeline producer should attach");
    let timeline_consumer = timeline
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("timeline consumer should attach");

    let audit = LocalInsightChannel::<DemoAuditInsightProtocol, 16>::new(
        InsightChannelClass::State,
        InsightCaptureMode::Exact,
    )
    .expect("allocator_audit requires debug-insights to be enabled");
    let audit_producer = audit
        .attach_producer(TransportAttachmentRequest::same_courier())
        .expect("audit producer should attach");
    let audit_consumer = audit
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("audit consumer should attach");

    let service = DemoService::new(allocator).expect("allocator service should build");
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

    let mut state = Box::pin(DemoState {
        allocator,
        service,
        default_domain,
        timeline,
        timeline_producer,
        audit,
        audit_producer,
        phase: DemoPhase::Metadata,
    });
    let mut member_cache: Vec<(MemoryPoolMemberId, MemoryPoolMemberInfo)> = Vec::new();

    let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
    let stack = FiberStack::from_slice(&mut stack_words).expect("fiber stack should build");
    let mut fiber = ManagedFiber::<_, 16>::new_with_publication(state.as_mut(), stack)
        .expect("allocator demo fiber should build");
    let fiber_metadata_consumer = fiber
        .metadata_channel()
        .expect("allocator demo fiber should expose explicit publication")
        .attach_consumer(TransportAttachmentRequest::same_courier())
        .expect("fiber metadata consumer should attach");

    print_rule("Allocator Audit Demo");
    println!("domain: {}", allocator_domain_id_value(default_domain));
    println!("fiber: id={} state=created", fiber.id().get());
    println!(
        "timeline insight: availability={} class={} capture={}",
        insight_availability_name(fiber.state().timeline.insight_support().availability),
        insight_channel_class_name(fiber.state().timeline.class()),
        insight_capture_mode_name(fiber.state().timeline.capture())
    );
    println!(
        "state insight: availability={} class={} capture={}",
        insight_availability_name(fiber.state().audit.insight_support().availability),
        insight_channel_class_name(fiber.state().audit.class()),
        insight_capture_mode_name(fiber.state().audit.capture())
    );
    fiber
        .state()
        .service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainPoolMembers {
                domain: default_domain,
            },
        )
        .expect("initial pool-members request should send");
    fiber
        .state()
        .service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainPoolExtents {
                domain: default_domain,
            },
        )
        .expect("initial pool-extents request should send");
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );

    let first = fiber.resume().expect("metadata fiber pump should resume");
    print_rule("Fiber Step 1");
    print_fiber_yield("fiber", first);
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );
    drain_metadata(fiber.state().service.metadata_channel(), metadata_consumer);
    drain_status(
        fiber.state().service.status_channel(),
        status_consumer,
        &mut member_cache,
    );
    drain_timeline(&fiber.state().timeline, timeline_consumer);
    drain_audit(&fiber.state().audit, audit_consumer);

    fiber
        .state()
        .service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainAudit {
                domain: default_domain,
            },
        )
        .expect("audit request should send");
    let second = fiber.resume().expect("audit fiber pump should resume");
    print_rule("Fiber Step 2");
    print_fiber_yield("fiber", second);
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );
    drain_status(
        fiber.state().service.status_channel(),
        status_consumer,
        &mut member_cache,
    );
    drain_timeline(&fiber.state().timeline, timeline_consumer);
    drain_audit(&fiber.state().audit, audit_consumer);

    fiber
        .state()
        .service
        .control_channel()
        .try_send(
            control_producer,
            AllocatorControlRequest::ReadDomainPoolStats {
                domain: default_domain,
            },
        )
        .expect("pool-stats request should send");
    let third = fiber.resume().expect("pool-stats fiber pump should resume");
    print_rule("Fiber Step 3");
    print_fiber_yield("fiber", third);
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );
    drain_status(
        fiber.state().service.status_channel(),
        status_consumer,
        &mut member_cache,
    );
    drain_timeline(&fiber.state().timeline, timeline_consumer);
    drain_audit(&fiber.state().audit, audit_consumer);

    let fourth = fiber
        .resume()
        .expect("pool-members fiber pump should resume");
    print_rule("Fiber Step 4");
    print_fiber_yield("fiber", fourth);
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );
    drain_status(
        fiber.state().service.status_channel(),
        status_consumer,
        &mut member_cache,
    );
    drain_timeline(&fiber.state().timeline, timeline_consumer);
    drain_audit(&fiber.state().audit, audit_consumer);

    let fifth = fiber
        .resume()
        .expect("pool-extents fiber pump should resume");
    print_rule("Fiber Step 5");
    print_fiber_yield("fiber", fifth);
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );
    drain_status(
        fiber.state().service.status_channel(),
        status_consumer,
        &mut member_cache,
    );
    drain_timeline(&fiber.state().timeline, timeline_consumer);
    drain_audit(&fiber.state().audit, audit_consumer);

    fiber
        .state()
        .service
        .control_channel()
        .try_send(control_producer, AllocatorControlRequest::RepublishDomains)
        .expect("republish request should send");
    let sixth = fiber.resume().expect("republish fiber pump should resume");
    print_rule("Fiber Step 6");
    print_fiber_yield("fiber", sixth);
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );
    drain_metadata(fiber.state().service.metadata_channel(), metadata_consumer);
    drain_status(
        fiber.state().service.status_channel(),
        status_consumer,
        &mut member_cache,
    );
    drain_timeline(&fiber.state().timeline, timeline_consumer);
    drain_audit(&fiber.state().audit, audit_consumer);

    let final_outcome = fiber.resume().expect("final fiber pump should complete");
    print_rule("Fiber Final");
    print_fiber_yield("fiber", final_outcome);
    drain_fiber_metadata(
        fiber
            .metadata_channel()
            .expect("allocator demo fiber should expose explicit publication"),
        fiber_metadata_consumer,
    );
    drain_status(
        fiber.state().service.status_channel(),
        status_consumer,
        &mut member_cache,
    );
    drain_timeline(&fiber.state().timeline, timeline_consumer);
    drain_audit(&fiber.state().audit, audit_consumer);

    match final_outcome {
        FiberYield::Completed(FiberReturn { code: 0 }) => {
            match fiber
                .state()
                .service
                .status_channel()
                .try_receive(status_consumer)
                .expect("final status receive should work")
            {
                Some(AllocatorControlStatusMessage::MetadataRepublishScheduled) | None => {}
                Some(other) => print_status_message(other, &mut member_cache),
            }
            match fiber
                .state()
                .service
                .metadata_channel()
                .try_receive(metadata_consumer)
                .expect("final metadata receive should work")
            {
                Some(AllocatorDomainMetadataMessage::Advertised(info)) => {
                    print_rule("Final Metadata");
                    print_metadata_message(AllocatorDomainMetadataMessage::Advertised(info));
                }
                Some(other) => print_metadata_message(other),
                None => {}
            }
        }
        other => {
            eprintln!("allocator_audit: unexpected fiber outcome");
            print_fiber_yield("fiber", other);
        }
    }
}
