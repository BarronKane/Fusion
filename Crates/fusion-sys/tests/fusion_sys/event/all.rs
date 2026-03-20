extern crate std;

use core::time::Duration;

use fusion_sys::event::{
    EventCaps,
    EventCompletionOp,
    EventCompletionOpKind,
    EventErrorKind,
    EventModel,
    EventSourceHandle,
    EventSystem,
};

#[test]
fn support_surface_is_exposed() {
    let event = EventSystem::new();
    let support = event.support();

    match support.model {
        EventModel::Unsupported => {
            assert!(support.caps.is_empty());
        }
        EventModel::Readiness | EventModel::Hybrid => {
            assert!(support.caps.contains(EventCaps::READINESS));
        }
        EventModel::Completion => {
            assert!(support.caps.contains(EventCaps::COMPLETION));
        }
    }
}

#[test]
fn create_and_submit_follow_backend_truth() {
    let event = EventSystem::new();
    let support = event.support();
    let poller = event.create();

    if support.model == EventModel::Unsupported {
        assert_eq!(
            poller
                .expect_err("unsupported backend should reject poller creation")
                .kind(),
            EventErrorKind::Unsupported
        );
        return;
    }

    let mut poller = poller.expect("supported backend should create a poller");
    let submit = event.submit(
        &mut poller,
        EventCompletionOp {
            source: EventSourceHandle(0),
            kind: EventCompletionOpKind::Custom(7),
            user_data: 99,
        },
    );

    if support.caps.contains(EventCaps::SUBMIT) {
        assert!(submit.is_ok());
    } else {
        assert_eq!(
            submit
                .expect_err("unsupported backend should reject completion submission")
                .kind(),
            EventErrorKind::Unsupported
        );
    }
}

#[test]
fn poll_rejects_empty_event_buffer_on_supported_backends() {
    let event = EventSystem::new();
    let support = event.support();
    let poller = event.create();

    if support.model == EventModel::Unsupported {
        assert_eq!(
            poller
                .expect_err("unsupported backend should reject poller creation")
                .kind(),
            EventErrorKind::Unsupported
        );
        return;
    }

    let mut poller = poller.expect("supported backend should create a poller");
    let result = event.poll(&mut poller, &mut [], Some(Duration::from_millis(1)));
    assert_eq!(
        result
            .expect_err("supported backend should reject empty poll buffers")
            .kind(),
        EventErrorKind::Invalid
    );
}
