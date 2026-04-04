const fn fiber_error_from_thread_pool(error: super::ThreadPoolError) -> FiberError {
    match error.kind() {
        fusion_sys::thread::ThreadErrorKind::Unsupported => FiberError::unsupported(),
        fusion_sys::thread::ThreadErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        fusion_sys::thread::ThreadErrorKind::Busy
        | fusion_sys::thread::ThreadErrorKind::Timeout
        | fusion_sys::thread::ThreadErrorKind::StateConflict => FiberError::state_conflict(),
        fusion_sys::thread::ThreadErrorKind::Invalid
        | fusion_sys::thread::ThreadErrorKind::PermissionDenied
        | fusion_sys::thread::ThreadErrorKind::PlacementDenied
        | fusion_sys::thread::ThreadErrorKind::SchedulerDenied
        | fusion_sys::thread::ThreadErrorKind::StackDenied
        | fusion_sys::thread::ThreadErrorKind::Platform(_) => FiberError::invalid(),
    }
}

const fn fiber_error_from_sync(error: SyncError) -> FiberError {
    match error.kind {
        SyncErrorKind::Unsupported => FiberError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => FiberError::invalid(),
        SyncErrorKind::Busy | SyncErrorKind::PermissionDenied | SyncErrorKind::Platform(_) => {
            FiberError::state_conflict()
        }
    }
}

const fn fiber_error_from_mem(error: fusion_pal::sys::mem::MemError) -> FiberError {
    match error.kind {
        fusion_pal::sys::mem::MemErrorKind::Unsupported => FiberError::unsupported(),
        fusion_pal::sys::mem::MemErrorKind::InvalidInput
        | fusion_pal::sys::mem::MemErrorKind::InvalidAddress
        | fusion_pal::sys::mem::MemErrorKind::Misaligned
        | fusion_pal::sys::mem::MemErrorKind::OutOfBounds
        | fusion_pal::sys::mem::MemErrorKind::PermissionDenied
        | fusion_pal::sys::mem::MemErrorKind::Overflow => FiberError::invalid(),
        fusion_pal::sys::mem::MemErrorKind::OutOfMemory => FiberError::resource_exhausted(),
        fusion_pal::sys::mem::MemErrorKind::Busy
        | fusion_pal::sys::mem::MemErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}

const fn fiber_error_from_event(error: fusion_sys::event::EventError) -> FiberError {
    match error.kind() {
        fusion_sys::event::EventErrorKind::Unsupported => FiberError::unsupported(),
        fusion_sys::event::EventErrorKind::Invalid => FiberError::invalid(),
        fusion_sys::event::EventErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        fusion_sys::event::EventErrorKind::Busy
        | fusion_sys::event::EventErrorKind::Timeout
        | fusion_sys::event::EventErrorKind::StateConflict
        | fusion_sys::event::EventErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}

const fn fiber_error_from_host(error: FiberHostError) -> FiberError {
    match error.kind() {
        FiberHostErrorKind::Unsupported => FiberError::unsupported(),
        FiberHostErrorKind::Invalid => FiberError::invalid(),
        FiberHostErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        FiberHostErrorKind::StateConflict | FiberHostErrorKind::Platform(_) => {
            FiberError::state_conflict()
        }
    }
}
