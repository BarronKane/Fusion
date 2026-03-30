//! macOS fusion-pal event backend built on `kqueue`.
//!
//! This backend exposes readiness-oriented polling through Darwin `kqueue`.
//! Completion submission remains unsupported.

use core::mem::MaybeUninit;
use core::time::Duration;

use crate::contract::pal::runtime::event::{
    EventBase,
    EventCaps,
    EventCompletionOp,
    EventError,
    EventInterest,
    EventKey,
    EventModel,
    EventNotification,
    EventReadiness,
    EventRecord,
    EventSource,
    EventSourceHandle,
    EventSupport,
};

const KQUEUE_BATCH: usize = 64;

const MACOS_EVENT_SUPPORT: EventSupport = EventSupport {
    caps: EventCaps::READINESS
        .union(EventCaps::LEVEL_TRIGGERED)
        .union(EventCaps::TIMEOUT),
    model: EventModel::Readiness,
    max_events: None,
    implementation: crate::contract::pal::runtime::event::EventImplementationKind::Native,
};

/// macOS readiness-poller provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsEvent;

/// macOS owned `kqueue` poller.
#[derive(Debug)]
pub struct MacOsPoller {
    kqueue_fd: libc::c_int,
}

/// Selected macOS event provider type.
pub type PlatformEvent = MacOsEvent;
/// Selected macOS poller type.
pub type PlatformPoller = MacOsPoller;

/// Returns the selected macOS event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}

impl MacOsEvent {
    /// Creates a new macOS event provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl EventBase for MacOsEvent {
    type Poller = MacOsPoller;

    fn support(&self) -> EventSupport {
        MACOS_EVENT_SUPPORT
    }
}

impl EventSource for MacOsEvent {
    fn create(&self) -> Result<Self::Poller, EventError> {
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(map_errno(last_errno()));
        }

        Ok(MacOsPoller { kqueue_fd: fd })
    }

    fn register(
        &self,
        poller: &mut Self::Poller,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        let fd = fd_from_source(source)?;
        apply_interest_change(poller.kqueue_fd, fd, interest, false)?;
        Ok(EventKey(source.0 as u64))
    }

    fn reregister(
        &self,
        poller: &mut Self::Poller,
        key: EventKey,
        interest: EventInterest,
    ) -> Result<(), EventError> {
        let fd = fd_from_key(key)?;
        apply_interest_change(poller.kqueue_fd, fd, interest, true)
    }

    fn deregister(&self, poller: &mut Self::Poller, key: EventKey) -> Result<(), EventError> {
        let fd = fd_from_key(key)?;
        remove_filter_if_present(poller.kqueue_fd, fd, libc::EVFILT_READ)?;
        remove_filter_if_present(poller.kqueue_fd, fd, libc::EVFILT_WRITE)?;
        Ok(())
    }

    fn submit(
        &self,
        _poller: &mut Self::Poller,
        _operation: EventCompletionOp,
    ) -> Result<EventKey, EventError> {
        Err(EventError::unsupported())
    }

    fn poll(
        &self,
        poller: &mut Self::Poller,
        events: &mut [EventRecord],
        timeout: Option<Duration>,
    ) -> Result<usize, EventError> {
        if events.is_empty() {
            return Err(EventError::invalid());
        }

        let timeout_storage = match timeout {
            Some(duration) => Some(duration_to_timespec(duration)?),
            None => None,
        };
        let timeout_ptr = timeout_storage
            .as_ref()
            .map_or(core::ptr::null(), core::ptr::from_ref);

        let batch_len = events.len().min(KQUEUE_BATCH);
        let batch_len_i32 = i32::try_from(batch_len).map_err(|_| EventError::invalid())?;
        let mut raw = [MaybeUninit::<libc::kevent>::uninit(); KQUEUE_BATCH];

        let ready = unsafe {
            libc::kevent(
                poller.kqueue_fd,
                core::ptr::null(),
                0,
                raw.as_mut_ptr().cast::<libc::kevent>(),
                batch_len_i32,
                timeout_ptr,
            )
        };

        if ready < 0 {
            let errno = last_errno();
            if errno == libc::EINTR {
                return Ok(0);
            }
            return Err(map_errno(errno));
        }

        let ready = usize::try_from(ready).map_err(|_| EventError::platform(libc::EOVERFLOW))?;
        for (index, slot) in raw.iter().take(ready).enumerate() {
            let event = unsafe { slot.assume_init() };
            events[index] = EventRecord {
                key: EventKey(event.ident as u64),
                notification: EventNotification::Readiness(readiness_from_kevent(event)),
            };
        }

        Ok(ready)
    }
}

impl Drop for MacOsPoller {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.kqueue_fd);
        }
    }
}

fn apply_interest_change(
    kqueue_fd: libc::c_int,
    fd: libc::c_int,
    interest: EventInterest,
    _replace: bool,
) -> Result<(), EventError> {
    validate_interest(interest)?;

    let mut changes = [empty_kevent(); 2];
    let mut len = 0_usize;

    let wants_read = interest.contains(EventInterest::READABLE);
    let wants_write = interest.contains(EventInterest::WRITABLE);

    changes[len] = make_change(fd, libc::EVFILT_READ, if wants_read { libc::EV_ADD } else { libc::EV_DELETE });
    len += 1;
    changes[len] = make_change(
        fd,
        libc::EVFILT_WRITE,
        if wants_write { libc::EV_ADD } else { libc::EV_DELETE },
    );
    len += 1;

    apply_changes_allow_delete_absent(kqueue_fd, &changes[..len])
}

fn validate_interest(interest: EventInterest) -> Result<(), EventError> {
    if interest.contains(EventInterest::PRIORITY) {
        return Err(EventError::unsupported());
    }

    if !(interest.contains(EventInterest::READABLE) || interest.contains(EventInterest::WRITABLE)) {
        return Err(EventError::invalid());
    }

    Ok(())
}

fn remove_filter_if_present(
    kqueue_fd: libc::c_int,
    fd: libc::c_int,
    filter: libc::c_short,
) -> Result<(), EventError> {
    let change = [make_change(fd, filter, libc::EV_DELETE)];
    match apply_changes_allow_delete_absent(kqueue_fd, &change) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == crate::contract::pal::runtime::event::EventErrorKind::StateConflict => Ok(()),
        Err(error) => Err(error),
    }
}

fn apply_changes_allow_delete_absent(
    kqueue_fd: libc::c_int,
    changes: &[libc::kevent],
) -> Result<(), EventError> {
    let nchanges = i32::try_from(changes.len()).map_err(|_| EventError::invalid())?;

    // `kevent` may block while waiting for ready events when an output list is supplied.
    // Change-list application must be non-blocking, so force zero timeout.
    let immediate = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    let mut out = [empty_kevent(); 4];
    let out_capacity = i32::try_from(out.len()).map_err(|_| EventError::invalid())?;

    let ready = unsafe {
        libc::kevent(
            kqueue_fd,
            changes.as_ptr(),
            nchanges,
            out.as_mut_ptr(),
            out_capacity,
            &raw const immediate,
        )
    };

    if ready < 0 {
        return Err(map_errno(last_errno()));
    }

    let ready = usize::try_from(ready).map_err(|_| EventError::platform(libc::EOVERFLOW))?;
    for event in out.iter().take(ready) {
        if event.flags & libc::EV_ERROR as u16 != 0 {
            if event.data == 0 {
                continue;
            }
            // `EV_DELETE` on a missing filter reports `ENOENT`; treat that as benign.
            if event.data == libc::ENOENT as isize {
                continue;
            }
            let code = i32::try_from(event.data).unwrap_or(libc::EOVERFLOW);
            return Err(map_errno(code));
        }
    }

    Ok(())
}

const fn make_change(fd: libc::c_int, filter: libc::c_short, flags: libc::c_ushort) -> libc::kevent {
    libc::kevent {
        ident: fd as libc::uintptr_t,
        filter,
        flags,
        fflags: 0,
        data: 0,
        udata: core::ptr::null_mut(),
    }
}

const fn empty_kevent() -> libc::kevent {
    libc::kevent {
        ident: 0,
        filter: 0,
        flags: 0,
        fflags: 0,
        data: 0,
        udata: core::ptr::null_mut(),
    }
}

fn readiness_from_kevent(event: libc::kevent) -> EventReadiness {
    let mut readiness = EventReadiness::empty();

    if event.filter == libc::EVFILT_READ {
        readiness |= EventReadiness::READABLE;
    }
    if event.filter == libc::EVFILT_WRITE {
        readiness |= EventReadiness::WRITABLE;
    }
    if event.flags & libc::EV_EOF as u16 != 0 {
        readiness |= EventReadiness::HANGUP;
    }
    if event.flags & libc::EV_ERROR as u16 != 0 || event.fflags != 0 {
        readiness |= EventReadiness::ERROR;
    }

    readiness
}

fn fd_from_source(source: EventSourceHandle) -> Result<libc::c_int, EventError> {
    i32::try_from(source.0).map_err(|_| EventError::invalid())
}

fn fd_from_key(key: EventKey) -> Result<libc::c_int, EventError> {
    i32::try_from(key.0).map_err(|_| EventError::invalid())
}

fn duration_to_timespec(duration: Duration) -> Result<libc::timespec, EventError> {
    let seconds = i64::try_from(duration.as_secs()).map_err(|_| EventError::invalid())?;
    let nanos = i64::from(duration.subsec_nanos());
    Ok(libc::timespec {
        tv_sec: seconds,
        tv_nsec: nanos,
    })
}

const fn map_errno(errno: libc::c_int) -> EventError {
    match errno {
        libc::EINVAL | libc::EBADF => EventError::invalid(),
        libc::ENOENT | libc::EEXIST => EventError::state_conflict(),
        libc::ENOMEM | libc::EMFILE | libc::ENFILE => EventError::resource_exhausted(),
        libc::EAGAIN => EventError::busy(),
        libc::ETIMEDOUT => EventError::timeout(),
        _ => EventError::platform(errno),
    }
}

fn last_errno() -> libc::c_int {
    unsafe { *libc::__error() }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use core::time::Duration;

    use super::*;

    #[test]
    fn macos_event_support_is_readiness_native() {
        let support = system_event().support();
        assert_eq!(support.model, EventModel::Readiness);
        assert_eq!(
            support.implementation,
            crate::contract::pal::runtime::event::EventImplementationKind::Native
        );
        assert!(support.caps.contains(EventCaps::READINESS));
        assert!(support.caps.contains(EventCaps::LEVEL_TRIGGERED));
        assert!(support.caps.contains(EventCaps::TIMEOUT));
        assert!(!support.caps.contains(EventCaps::COMPLETION));
    }

    #[test]
    fn macos_event_submit_is_unsupported() {
        let provider = system_event();
        let mut poller = provider.create().expect("poller should create");
        let error = provider
            .submit(
                &mut poller,
                EventCompletionOp {
                    source: EventSourceHandle(0),
                    kind: crate::contract::pal::runtime::event::EventCompletionOpKind::Read,
                    user_data: 0,
                },
            )
            .expect_err("completion submission should be unsupported");
        assert_eq!(
            error.kind(),
            crate::contract::pal::runtime::event::EventErrorKind::Unsupported
        );
    }

    #[test]
    fn macos_event_poll_rejects_empty_output_buffer() {
        let provider = system_event();
        let mut poller = provider.create().expect("poller should create");
        let mut events: [EventRecord; 0] = [];
        let error = provider
            .poll(&mut poller, &mut events, Some(Duration::from_millis(1)))
            .expect_err("empty output buffer should be rejected");
        assert_eq!(
            error.kind(),
            crate::contract::pal::runtime::event::EventErrorKind::Invalid
        );
    }

    #[test]
    fn macos_event_registers_pipe_readiness() {
        let provider = system_event();
        let mut poller = provider.create().expect("poller should create");

        let mut fds = [0_i32; 2];
        let pipe_rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(pipe_rc, 0, "pipe should be created");
        let read_fd = fds[0];
        let write_fd = fds[1];

        let key = provider
            .register(
                &mut poller,
                EventSourceHandle(read_fd as usize),
                EventInterest::READABLE,
            )
            .expect("pipe read-end should register");

        let payload = [0x7Fu8; 1];
        let write_rc =
            unsafe { libc::write(write_fd, payload.as_ptr().cast::<libc::c_void>(), payload.len()) };
        assert_eq!(write_rc, 1, "write should signal readability");

        let mut events = [EventRecord {
            key: EventKey(0),
            notification: EventNotification::Readiness(EventReadiness::empty()),
        }; 4];
        let count = provider
            .poll(&mut poller, &mut events, Some(Duration::from_millis(100)))
            .expect("poll should evaluate");
        assert!(count >= 1, "at least one readiness event should be returned");
        assert!(
            events[..count].iter().any(|record| {
                record.key == key
                    && matches!(
                        record.notification,
                        EventNotification::Readiness(readiness)
                            if readiness.contains(EventReadiness::READABLE)
                    )
            }),
            "registered pipe should report readable readiness"
        );

        provider
            .deregister(&mut poller, key)
            .expect("deregister should succeed");
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
}
