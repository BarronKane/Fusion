extern crate std;

use core::time::Duration;

use fusion_sys::event::{
    EventCaps,
    EventInterest,
    EventModel,
    EventNotification,
    EventReadiness,
    EventRecord,
    EventRegistration,
    EventRegistrationMode,
    EventSourceHandle,
    EventSystem,
};

#[derive(Debug)]
struct TestPipe {
    read_fd: i32,
    write_fd: i32,
}

impl TestPipe {
    fn new() -> Self {
        let mut fds = [0_i32; 2];
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        assert_eq!(rc, 0, "nonblocking test pipe should create");
        Self {
            read_fd: fds[0],
            write_fd: fds[1],
        }
    }

    fn source(&self) -> EventSourceHandle {
        EventSourceHandle(usize::try_from(self.read_fd).expect("pipe fd should be non-negative"))
    }

    fn write_byte(&self, value: u8) {
        let rc = unsafe {
            libc::write(
                self.write_fd,
                (&raw const value).cast::<libc::c_void>(),
                core::mem::size_of::<u8>(),
            )
        };
        assert_eq!(rc, 1, "pipe writer should make the reader readable");
    }
}

impl Drop for TestPipe {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

#[test]
fn linux_event_support_reports_native_readiness_backend() {
    let support = EventSystem::new().support();

    assert_eq!(support.model, EventModel::Readiness);
    assert_eq!(
        support.implementation,
        fusion_sys::event::EventImplementationKind::Native
    );
    assert!(support.caps.contains(EventCaps::READINESS));
    assert!(support.caps.contains(EventCaps::LEVEL_TRIGGERED));
    assert!(support.caps.contains(EventCaps::TIMEOUT));
    assert!(!support.caps.contains(EventCaps::COMPLETION));
    assert!(!support.caps.contains(EventCaps::SUBMIT));
}

#[test]
fn linux_event_poller_reports_readable_pipes() {
    let event = EventSystem::new();
    let mut poller = event.create().expect("poller should create");
    let pipe = TestPipe::new();

    let key = event
        .register(
            &mut poller,
            pipe.source(),
            EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
        )
        .expect("pipe reader should register");

    pipe.write_byte(b'x');

    let mut events = [EventRecord {
        key,
        notification: EventNotification::Readiness(EventReadiness::empty()),
    }; 4];
    let ready = event
        .poll(&mut poller, &mut events, Some(Duration::from_secs(1)))
        .expect("poll should succeed");
    assert!(ready >= 1);

    let record = &events[0];
    assert_eq!(record.key, key);
    let readiness = match record.notification {
        EventNotification::Readiness(readiness) => readiness,
        EventNotification::Completion(_) => {
            panic!("linux epoll backend should not emit completions")
        }
    };
    assert!(readiness.contains(EventReadiness::READABLE));

    event
        .deregister(&mut poller, key)
        .expect("deregister should succeed");
}

#[test]
fn linux_event_register_with_non_default_mode_is_honestly_unsupported() {
    let event = EventSystem::new();
    let mut poller = event.create().expect("poller should create");
    let pipe = TestPipe::new();

    let error = event
        .register_with(
            &mut poller,
            EventRegistration {
                source: pipe.source(),
                interest: EventInterest::READABLE,
                mode: EventRegistrationMode::OneShot,
            },
        )
        .expect_err("linux backend should not pretend one-shot support through the generic path");

    assert_eq!(error.kind(), fusion_sys::event::EventErrorKind::Unsupported);
}
