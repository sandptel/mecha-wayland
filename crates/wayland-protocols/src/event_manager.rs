pub use crate::spmc_queue::EventImpl;
use crate::spmc_queue::SpmcQueue;

/// Handler storage and cursor-based draining are added in the next stage.
pub struct EventTopic<E: EventImpl, const QUEUE_CAP: usize, const MAX_HANDLERS: usize> {
    queue: SpmcQueue<E>,
    handlers: [Option<HandlerEntry<E>>; MAX_HANDLERS],
    cursors: [usize; MAX_HANDLERS],
    len: usize,
}

impl<E: EventImpl, const QUEUE_CAP: usize, const MAX_HANDLERS: usize>
    EventTopic<E, QUEUE_CAP, MAX_HANDLERS>
{
    pub fn new() -> Self {
        Self {
            queue: SpmcQueue::with_capacity_pow2(QUEUE_CAP),
            handlers: [None; MAX_HANDLERS],
            cursors: [0; MAX_HANDLERS],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn emit(&self, event: E) {
        self.queue.push(event);
    }

    #[inline(always)]
    pub fn queue(&self) -> &SpmcQueue<E> {
        &self.queue
    }

    pub fn register<H>(&mut self, handler: &mut H) -> Result<usize, RegisterHandlerError>
    where
        H: EventHandler<E> + 'static,
    {
        if self.len >= MAX_HANDLERS {
            return Err(RegisterHandlerError::Full);
        }

        let slot = self.len;
        self.handlers[slot] = Some(HandlerEntry::from_handler(handler));
        self.cursors[slot] = self.queue.current_write();
        self.len += 1;
        Ok(slot)
    }

    pub fn handler_count(&self) -> usize {
        self.len
    }

    /// Drain queued events and dispatch to all registered handlers.
    ///
    /// Overflow policy: if a handler lags beyond queue capacity, the oldest
    /// unseen events are dropped for that handler.
    pub fn poll(&mut self) {
        let write = self.queue.current_write();
        let capacity = self.queue.capacity();

        for slot in 0..self.len {
            let Some(entry) = self.handlers[slot] else {
                continue;
            };

            let mut cursor = self.cursors[slot];

            if write.saturating_sub(cursor) > capacity {
                cursor = write - capacity;
            }

            while cursor < write {
                let event = unsafe { self.queue.read_unchecked(cursor) };
                unsafe {
                    (entry.call)(entry.ctx, event);
                }
                cursor += 1;
            }

            self.cursors[slot] = cursor;
        }
    }
}

// HandlerEntry exists to make typed handlers
// storable in a fixed array while still calling the correct handle_event implementation later
#[derive(Clone, Copy)]
struct HandlerEntry<E: EventImpl> {
    ctx: *mut (),
    call: unsafe fn(*mut (), &E),
}

impl<E: EventImpl> HandlerEntry<E> {
    fn from_handler<H>(handler: &mut H) -> Self
    where
        H: EventHandler<E> + 'static,
    {
        Self {
            ctx: handler as *mut H as *mut (),
            call: call_handler::<H, E>,
        }
    }
}

unsafe fn call_handler<H, E>(ctx: *mut (), event: &E)
where
    H: EventHandler<E>,
    E: EventImpl,
{
    unsafe {
        let handler = &mut *(ctx as *mut H);
        handler.handle_event(event);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterHandlerError {
    Full,
}

pub trait EventHandler<E: EventImpl> {
    fn handle_event(&mut self, event: &E);
}

/// Implemented internally for each event configured in `define_event_system!`.
pub trait EventRoute: EventImpl {
    fn ensure_init();
    fn emit_to_topic(event: Self);
    fn register_to_topic<H>(handler: &mut H) -> Result<usize, RegisterHandlerError>
    where
        H: EventHandler<Self> + 'static;
    fn poll_topic();
}

#[macro_export]
macro_rules! define_event_system {
    (
        $(
            $event:ty => {
                topic: $topic:ident,
                queue_capacity: $queue_cap:expr,
                max_handlers: $max_handlers:expr
            }
        ),* $(,)?
    ) => {
        $(
            static mut $topic: Option<$crate::event_manager::EventTopic<$event, $queue_cap, $max_handlers>> = None;

            impl $crate::event_manager::EventRoute for $event {
                #[allow(static_mut_refs)]
                fn ensure_init() {
                    unsafe {
                        if $topic.is_none() {
                            $topic = Some($crate::event_manager::EventTopic::new());
                        }
                    }
                }

                #[allow(static_mut_refs)]
                fn emit_to_topic(event: Self) {
                    unsafe {
                        if let Some(topic) = $topic.as_ref() {
                            topic.emit(event);
                        }
                    }
                }

                #[allow(static_mut_refs)]
                fn register_to_topic<H>(
                    handler: &mut H,
                ) -> Result<usize, $crate::event_manager::RegisterHandlerError>
                where
                    H: $crate::event_manager::EventHandler<Self> + 'static,
                {
                    unsafe {
                        if $topic.is_none() {
                            $topic = Some($crate::event_manager::EventTopic::new());
                        }

                        $topic
                            .as_mut()
                            .expect("topic must be initialized")
                            .register(handler)
                    }
                }

                #[allow(static_mut_refs)]
                fn poll_topic() {
                    unsafe {
                        if let Some(topic) = $topic.as_mut() {
                            topic.poll();
                        }
                    }
                }
            }
        )*

        pub struct EventSystem;

        impl EventSystem {
            pub fn init() {
                $(
                    <$event as $crate::event_manager::EventRoute>::ensure_init();
                )*
            }

            #[inline(always)]
            pub fn emit<E: $crate::event_manager::EventRoute>(event: E) {
                E::ensure_init();
                E::emit_to_topic(event);
            }

            pub fn register<E, H>(
                handler: &mut H,
            ) -> Result<usize, $crate::event_manager::RegisterHandlerError>
            where
                E: $crate::event_manager::EventRoute,
                H: $crate::event_manager::EventHandler<E> + 'static,
            {
                E::register_to_topic(handler)
            }

            pub fn poll<E: $crate::event_manager::EventRoute>() {
                E::poll_topic();
            }

            #[inline(always)]
            pub fn poll_all() {
                $(
                    <$event as $crate::event_manager::EventRoute>::poll_topic();
                )*
            }
        }
    };
}

define_event_system! {
    crate::wl_pointer::PointerEvent => {
        topic: POINTER_TOPIC,
        queue_capacity: 64,
        max_handlers: 8
    }
}

// EventSystem::emit(Event);
