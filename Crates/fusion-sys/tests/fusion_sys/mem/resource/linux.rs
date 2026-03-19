//! Linux-specific `fusion_sys::mem::resource` integration tests.
//!
//! These tests are free to use `std`, `libc`, `memfd`, and other Linux-specific facilities
//! because the module is only compiled on Linux. They exercise the current fusion-pal behavior rather
//! than a cross-platform contract.

use std::fs::File;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd};
use std::ptr::{read_volatile, write_bytes, write_volatile};

use fusion_pal::sys::mem::{BorrowedBackingHandle, Protect};
use fusion_sys::mem::resource::{
    AddressReservation, InitialResidency, MemoryResource, ProtectableResource, QueryableResource,
    RequiredPlacement, ReservationRequest, ResourceBackingKind, ResourceHazardSet,
    ResourcePreferenceSet, ResourceRange, ResourceRequest, StateValue, VirtualMemoryResource,
};
use rustix::fd::IntoRawFd;
use rustix::fs::{MemfdFlags, memfd_create};

use super::support::page_len;

fn baseline_heap_request(len: usize) -> ResourceRequest<'static> {
    ResourceRequest::anonymous_private(len)
}

fn locked_low_latency_request(len: usize) -> ResourceRequest<'static> {
    let mut request = ResourceRequest::anonymous_private(len);
    request.initial.residency = InitialResidency::Locked;
    request.preferences = ResourcePreferenceSet::PREFAULT | ResourcePreferenceSet::HUGE_PAGES;
    request
}

fn staged_executable_request(len: usize) -> ResourceRequest<'static> {
    let mut request = ResourceRequest::anonymous_private(len);
    request.contract.allowed_protect = Protect::READ | Protect::WRITE | Protect::EXEC;
    request
}

// Use memfd instead of `temp_dir()` paths so the file-backed tests do not depend on ambient
// filesystem layout or whether the temp directory supports `mmap` cleanly.
fn memory_backed_file(name: &str, len: usize) -> File {
    let fd = memfd_create(name, MemfdFlags::CLOEXEC).expect("memfd");
    let file = unsafe { File::from_raw_fd(fd.into_raw_fd()) };
    file.set_len(len as u64).expect("resize");
    file
}

// Minor faults count first-touch demand paging work that the kernel can satisfy without disk
// I/O. The page-fault test samples this counter around a write loop.
fn current_minor_faults() -> libc::c_long {
    let mut usage = MaybeUninit::<libc::rusage>::uninit();
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    assert_eq!(rc, 0, "getrusage should succeed");
    unsafe { usage.assume_init() }.ru_minflt
}

#[test]
fn baseline_private_heap_profile_maps_and_queries() {
    let resource = VirtualMemoryResource::create(&baseline_heap_request(page_len(4)))
        .expect("baseline heap should map");
    let base = resource.view().base_addr();
    let info = resource.query(base).expect("query");

    // The default Linux heap profile should be anonymous private RW memory with query support.
    assert_eq!(
        resource.backing_kind(),
        ResourceBackingKind::AnonymousPrivate
    );
    assert_eq!(
        resource.state().current_protect,
        StateValue::Uniform(Protect::READ | Protect::WRITE)
    );
    assert!(info.protect.contains(Protect::READ));
    assert!(info.protect.contains(Protect::WRITE));
}

#[test]
fn shared_ipc_profile_marks_aliasing_hazard() {
    let resource = VirtualMemoryResource::create(&ResourceRequest::anonymous_shared(page_len(4)))
        .expect("shared resource should map");

    // Shared anonymous mappings should surface the aliasing hazard explicitly.
    assert_eq!(
        resource.backing_kind(),
        ResourceBackingKind::AnonymousShared
    );
    assert!(
        resource
            .hazards()
            .contains(ResourceHazardSet::SHARED_ALIASING)
    );
}

#[test]
fn locked_low_latency_profile_verifies_lock_state() {
    let resource = VirtualMemoryResource::create(&locked_low_latency_request(page_len(4)))
        .expect("locked resource should map");

    // `InitialResidency::Locked` is a hard requirement, so the live state should reflect an
    // actually verified lock rather than a mere request bit.
    assert_eq!(resource.state().locked, StateValue::Uniform(true));
    assert!(
        !resource
            .resolved()
            .unmet_preferences
            .contains(ResourcePreferenceSet::LOCK)
    );
}

#[test]
fn staged_executable_profile_allows_wxorx_transition() {
    let resource = VirtualMemoryResource::create(&staged_executable_request(page_len(2)))
        .expect("staged executable resource should map");
    let whole = ResourceRange::whole(resource.len());

    // The initial contract allows EXEC, but W^X means we stage the transition instead of
    // mapping RWX up front.
    unsafe { resource.protect(whole, Protect::READ | Protect::EXEC) }
        .expect("w^x transition should succeed");

    assert_eq!(
        resource.state().current_protect,
        StateValue::Uniform(Protect::READ | Protect::EXEC)
    );
}

#[test]
fn reservation_materialization_profile_lands_in_reserved_window() {
    let page = page_len(1);
    let reservation =
        AddressReservation::create(&ReservationRequest::new(page * 3)).expect("reservation");
    let target = reservation
        .subview(ResourceRange::new(page, page))
        .expect("target subview");
    let target_base = target.base_addr();
    let target_len = target.len();

    let mut request = baseline_heap_request(page);
    request.required_placement = Some(RequiredPlacement::FixedNoReplace(target_base.get()));

    // Materialization should land exactly in the requested hole and split the reservation
    // around it.
    let materialized = reservation
        .materialize_range(ResourceRange::new(page, page), &request)
        .expect("materialized");

    assert_eq!(materialized.resource.view().base_addr(), target_base);
    assert_eq!(materialized.resource.len(), target_len);
    assert_eq!(
        materialized
            .leading
            .as_ref()
            .expect("leading reservation")
            .view()
            .len(),
        page
    );
    assert_eq!(
        materialized
            .trailing
            .as_ref()
            .expect("trailing reservation")
            .view()
            .len(),
        page
    );
}

#[test]
fn file_backed_private_profile_maps_memfd_object() {
    let page = page_len(2);
    let file = memory_backed_file("fusion-sys-file-profile", page);

    // `file_private` exercises the file-backed path without depending on a visible path or
    // shared filesystem state.
    let request = ResourceRequest::file_private(
        page,
        unsafe { BorrowedBackingHandle::borrow_raw_fd(file.as_raw_fd()) }
            .expect("valid memfd handle"),
        0,
    );
    let resource = VirtualMemoryResource::create(&request).expect("file-backed resource");
    let base = resource.view().base_addr();
    let info = resource.query(base).expect("query");

    assert_eq!(resource.backing_kind(), ResourceBackingKind::FilePrivate);
    assert_eq!(
        resource.state().current_protect,
        StateValue::Uniform(Protect::READ | Protect::WRITE)
    );
    assert!(info.protect.contains(Protect::READ));
    assert!(info.protect.contains(Protect::WRITE));

    drop(resource);
    drop(file);
}

#[test]
fn gigabyte_huge_page_memfd_profile_tracks_asymmetric_protection() {
    const GIB: usize = 1024 * 1024 * 1024;

    let page = page_len(1);
    let file = memory_backed_file("fusion-sys-gib-file-profile", GIB);

    // Linux only exposes huge pages here as a preference/advice path, so this test focuses on
    // state tracking over a large file-backed range rather than trying to prove huge-page
    // backing itself.
    let mut request = ResourceRequest::file_private(
        GIB,
        unsafe { BorrowedBackingHandle::borrow_raw_fd(file.as_raw_fd()) }
            .expect("valid memfd handle"),
        0,
    );
    request.preferences = ResourcePreferenceSet::HUGE_PAGES;

    let resource = VirtualMemoryResource::create(&request).expect("gigabyte file-backed resource");
    let first_half = ResourceRange::new(0, GIB / 2);
    let second_half = resource
        .view()
        .subrange(ResourceRange::new(GIB / 2, page))
        .expect("second-half subrange");

    // A partial protection update should force the summary state to become asymmetric while
    // point queries remain truthful.
    unsafe { resource.protect(first_half, Protect::READ) }.expect("partial protect should succeed");

    assert_eq!(resource.backing_kind(), ResourceBackingKind::FilePrivate);
    assert_eq!(resource.state().current_protect, StateValue::Asymmetric);

    let first_info = resource
        .query(resource.view().base_addr())
        .expect("first query");
    let second_info = resource
        .query(second_half.base_addr())
        .expect("second query");

    assert_eq!(first_info.protect, Protect::READ);
    assert_eq!(second_info.protect, Protect::READ | Protect::WRITE);

    drop(resource);
    drop(file);
}

#[test]
fn gigabyte_heap_profile_touches_pages_and_tracks_asymmetric_state() {
    const GIB: usize = 1024 * 1024 * 1024;
    const MIB: usize = 1024 * 1024;
    const TAIL_WRITE_BYTES: usize = 10 * MIB;

    let page = page_len(1);
    assert_eq!(GIB % page, 0, "test assumes 1 GiB is page-aligned");
    assert_eq!(
        TAIL_WRITE_BYTES % page,
        0,
        "test assumes 10 MiB is page-aligned"
    );

    let mut request = ResourceRequest::anonymous_private(GIB);
    request.preferences = ResourcePreferenceSet::HUGE_PAGES;

    let resource = VirtualMemoryResource::create(&request).expect("gigabyte heap resource");
    let half = GIB / 2;
    let upper_half = ResourceRange::new(half, half);
    let upper_page = resource
        .view()
        .subrange(ResourceRange::new(half, page))
        .expect("upper-half subrange");
    let base = unsafe { resource.view().base_ptr() };
    let tail_write_start = GIB - TAIL_WRITE_BYTES;
    let tail_write_probe = tail_write_start + page * 4;

    // Touch a contiguous tail span so the test causes real demand paging work without turning
    // into a tiny soak test.
    unsafe { write_bytes(base.add(tail_write_start), 0x01_u8, TAIL_WRITE_BYTES) };

    let sample_writes = [
        (0, 0x11_u8),
        (page, 0x22_u8),
        (half - page, 0x33_u8),
        (half, 0x44_u8),
        (half + page, 0x55_u8),
        (GIB - page, 0x66_u8),
    ];

    for (offset, value) in sample_writes {
        unsafe { write_volatile(base.add(offset), value) };
        let observed = unsafe { read_volatile(base.add(offset)) };
        assert_eq!(observed, value);
    }

    // The tail probe confirms that the wider bulk write actually dirtied a page near the end of
    // the mapping rather than only the hand-written sample offsets.
    let tail_probe_observed = unsafe { read_volatile(base.add(tail_write_probe)) };
    assert_eq!(tail_probe_observed, 0x01);

    // Protect only the upper half so the resource-wide summary can no longer be uniform.
    unsafe { resource.protect(upper_half, Protect::READ) }.expect("partial protect should succeed");

    assert_eq!(
        resource.backing_kind(),
        ResourceBackingKind::AnonymousPrivate
    );
    assert_eq!(resource.state().current_protect, StateValue::Asymmetric);

    let lower_info = resource
        .query(resource.view().base_addr())
        .expect("lower query");
    let upper_info = resource.query(upper_page.base_addr()).expect("upper query");

    assert_eq!(lower_info.protect, Protect::READ | Protect::WRITE);
    assert_eq!(upper_info.protect, Protect::READ);

    unsafe { write_volatile(base.add(page * 2), 0x7A_u8) };
    let lower_observed = unsafe { read_volatile(base.add(page * 2)) };
    assert_eq!(lower_observed, 0x7A);

    // Reads from the upper half should still succeed after the transition, but writes there
    // would fault, so the test only validates preserved contents.
    let tail_probe_after = unsafe { read_volatile(base.add(tail_write_probe)) };
    let upper_boundary = unsafe { read_volatile(base.add(half)) };
    let upper_tail = unsafe { read_volatile(base.add(GIB - page)) };
    assert_eq!(tail_probe_after, 0x01);
    assert_eq!(upper_boundary, 0x44);
    assert_eq!(upper_tail, 0x66);
}

#[test]
fn anonymous_mapping_touch_loop_increases_minor_fault_count() {
    let page = page_len(1);
    let pages_to_touch = 128_usize;
    let len = page * pages_to_touch;

    let resource =
        VirtualMemoryResource::create(&ResourceRequest::anonymous_private(len)).expect("resource");
    let base = unsafe { resource.view().base_ptr() };
    let before = current_minor_faults();

    // Anonymous private memory is mapped lazily. The first write into each untouched page should
    // trigger a minor fault so the kernel can allocate and map backing for that page.
    for page_index in 0..pages_to_touch {
        let offset = page_index * page;
        unsafe { write_volatile(base.add(offset), 0xA5_u8) };
    }

    let after = current_minor_faults();
    let delta = after - before;

    assert!(
        delta > 0,
        "touching previously untouched anonymous pages should raise minor faults"
    );

    let first = unsafe { read_volatile(base) };
    let last = unsafe { read_volatile(base.add((pages_to_touch - 1) * page)) };
    assert_eq!(first, 0xA5);
    assert_eq!(last, 0xA5);
}
