extern crate std;

use core::time::Duration;

use fusion_sys::event::{
    EventCaps, EventInterest, EventModel, EventNotification, EventReadiness, EventRecord,
    EventSourceHandle, EventSystem,
};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

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
fn linux_event_poller_reports_readable_unix_streams() {
    let event = EventSystem::new();
    let mut poller = event.create().expect("poller should create");
    let (reader, mut writer) = UnixStream::pair().expect("unix stream pair should create");

    let key = event
        .register(
            &mut poller,
            EventSourceHandle(
                usize::try_from(reader.as_raw_fd()).expect("unix stream fd should be non-negative"),
            ),
            EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
        )
        .expect("reader should register");

    writer
        .write_all(b"x")
        .expect("writer should make reader readable");

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
