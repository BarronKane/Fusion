//! Linux fusion-pal event backend built on `epoll`.
//!
//! The Linux backend exposes readiness-oriented polling honestly through `epoll`.
//! Completion submission remains unsupported here because that would be a different model,
//! not just a missing flag.

use core::mem::MaybeUninit;
use core::time::Duration;

use crate::pal::event::{
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

const EPOLL_BATCH: usize = 64;

const LINUX_EVENT_SUPPORT: EventSupport = EventSupport {
    caps: EventCaps::READINESS
        .union(EventCaps::LEVEL_TRIGGERED)
        .union(EventCaps::TIMEOUT),
    model: EventModel::Readiness,
    max_events: None,
    implementation: crate::pal::event::EventImplementationKind::Native,
};

/// Linux readiness-poller provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxEvent;

/// Linux owned `epoll` poller.
#[derive(Debug)]
pub struct LinuxPoller {
    epoll_fd: libc::c_int,
}

/// Selected Linux event provider type.
pub type PlatformEvent = LinuxEvent;
/// Selected Linux poller type.
pub type PlatformPoller = LinuxPoller;

/// Returns the selected Linux event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}

impl LinuxEvent {
    /// Creates a new Linux event provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl EventBase for LinuxEvent {
    type Poller = LinuxPoller;

    fn support(&self) -> EventSupport {
        LINUX_EVENT_SUPPORT
    }
}

impl EventSource for LinuxEvent {
    fn create(&self) -> Result<Self::Poller, EventError> {
        let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            return Err(map_errno(last_errno()));
        }
        Ok(LinuxPoller { epoll_fd })
    }

    fn register(
        &self,
        poller: &mut Self::Poller,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        let fd = fd_from_source(source)?;
        let key = EventKey(source.0 as u64);
        let mut event = epoll_event_for_interest(interest, key)?;
        let rc =
            unsafe { libc::epoll_ctl(poller.epoll_fd, libc::EPOLL_CTL_ADD, fd, &raw mut event) };
        if rc < 0 {
            return Err(map_errno(last_errno()));
        }
        Ok(key)
    }

    fn reregister(
        &self,
        poller: &mut Self::Poller,
        key: EventKey,
        interest: EventInterest,
    ) -> Result<(), EventError> {
        let fd = fd_from_key(key)?;
        let mut event = epoll_event_for_interest(interest, key)?;
        let rc =
            unsafe { libc::epoll_ctl(poller.epoll_fd, libc::EPOLL_CTL_MOD, fd, &raw mut event) };
        if rc < 0 {
            return Err(map_errno(last_errno()));
        }
        Ok(())
    }

    fn deregister(&self, poller: &mut Self::Poller, key: EventKey) -> Result<(), EventError> {
        let fd = fd_from_key(key)?;
        let rc = unsafe {
            libc::epoll_ctl(
                poller.epoll_fd,
                libc::EPOLL_CTL_DEL,
                fd,
                core::ptr::null_mut(),
            )
        };
        if rc < 0 {
            return Err(map_errno(last_errno()));
        }
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

        let mut total = 0;
        let mut timeout_ms = timeout_to_epoll(timeout);
        let mut raw = [MaybeUninit::<libc::epoll_event>::uninit(); EPOLL_BATCH];

        loop {
            let batch_len = events.len().saturating_sub(total).min(EPOLL_BATCH);
            if batch_len == 0 {
                return Ok(total);
            }
            let batch_len_c_int =
                libc::c_int::try_from(batch_len).map_err(|_| EventError::invalid())?;

            let ready = unsafe {
                libc::epoll_wait(
                    poller.epoll_fd,
                    raw.as_mut_ptr().cast::<libc::epoll_event>(),
                    batch_len_c_int,
                    timeout_ms,
                )
            };
            if ready < 0 {
                let errno = last_errno();
                if errno == libc::EINTR {
                    continue;
                }
                return Err(map_errno(errno));
            }
            if ready == 0 {
                return Ok(total);
            }

            let ready =
                usize::try_from(ready).map_err(|_| EventError::platform(libc::EOVERFLOW))?;
            for index in 0..ready {
                let raw_event = unsafe { raw[index].assume_init() };
                events[total + index] = EventRecord {
                    key: EventKey(raw_event.u64),
                    notification: EventNotification::Readiness(readiness_from_epoll(
                        raw_event.events,
                    )),
                };
            }
            total += ready;

            if ready < batch_len {
                return Ok(total);
            }

            timeout_ms = 0;
        }
    }
}

impl Drop for LinuxPoller {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.epoll_fd);
        }
    }
}

fn fd_from_source(source: EventSourceHandle) -> Result<libc::c_int, EventError> {
    i32::try_from(source.0).map_err(|_| EventError::invalid())
}

fn fd_from_key(key: EventKey) -> Result<libc::c_int, EventError> {
    i32::try_from(key.0).map_err(|_| EventError::invalid())
}

const fn epoll_event_for_interest(
    interest: EventInterest,
    key: EventKey,
) -> Result<libc::epoll_event, EventError> {
    let mut events = 0_u32;
    if interest.contains(EventInterest::READABLE) {
        events |= libc::EPOLLIN as u32;
    }
    if interest.contains(EventInterest::WRITABLE) {
        events |= libc::EPOLLOUT as u32;
    }
    if interest.contains(EventInterest::PRIORITY) {
        events |= libc::EPOLLPRI as u32;
    }

    if events == 0 {
        return Err(EventError::invalid());
    }

    Ok(libc::epoll_event { events, u64: key.0 })
}

fn readiness_from_epoll(events: u32) -> EventReadiness {
    let mut readiness = EventReadiness::empty();
    if events & (libc::EPOLLIN as u32) != 0 {
        readiness |= EventReadiness::READABLE;
    }
    if events & (libc::EPOLLOUT as u32) != 0 {
        readiness |= EventReadiness::WRITABLE;
    }
    if events & (libc::EPOLLPRI as u32) != 0 {
        readiness |= EventReadiness::PRIORITY;
    }
    if events & (libc::EPOLLERR as u32) != 0 {
        readiness |= EventReadiness::ERROR;
    }
    if events & ((libc::EPOLLHUP as u32) | epoll_rdhup()) != 0 {
        readiness |= EventReadiness::HANGUP;
    }
    readiness
}

const fn epoll_rdhup() -> u32 {
    #[cfg(any(
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64",
        target_arch = "x86",
        target_arch = "x86_64"
    ))]
    {
        libc::EPOLLRDHUP as u32
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64",
        target_arch = "x86",
        target_arch = "x86_64"
    )))]
    {
        0
    }
}

fn timeout_to_epoll(timeout: Option<Duration>) -> libc::c_int {
    timeout.map_or(-1, |duration| {
        let millis = duration.as_millis();
        libc::c_int::try_from(millis).map_or(libc::c_int::MAX, |millis| millis)
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
    unsafe { *libc::__errno_location() }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate std;

    #[test]
    fn linux_event_support_is_readiness_native() {
        let support = system_event().support();
        assert_eq!(support.model, EventModel::Readiness);
        assert_eq!(
            support.implementation,
            crate::pal::event::EventImplementationKind::Native
        );
        assert!(support.caps.contains(EventCaps::READINESS));
        assert!(support.caps.contains(EventCaps::LEVEL_TRIGGERED));
        assert!(support.caps.contains(EventCaps::TIMEOUT));
        assert!(!support.caps.contains(EventCaps::COMPLETION));
    }
}
