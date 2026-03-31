use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(align(64))]
struct CacheAligned<T>(T);

// Currently just a marker trait to abstract Events
pub trait EventImpl: Copy + Send + Sync + 'static {}

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

// Example for EventTopic
// // One topic = one queue lane for PointerEvent.
// // 64 queue slots, reserve space conceptually for up to 8 handlers later.
// use std::sync::OnceLock;
// static POINTER_TOPIC: OnceLock<EventTopic<PointerEvent, 64, 8>> = OnceLock::new();

// pub struct EventSystem;

// impl EventSystem {
//     pub fn init() {
//         let _ = POINTER_TOPIC.set(EventTopic::new());
//     }

//     pub fn emit_pointer(event: PointerEvent) {
//         if let Some(topic) = POINTER_TOPIC.get() {
//             topic.emit(event);
//         }
//     }
// }

// Single-producer, multi-consumer ring queue primitive.
// Events -> Ring Buffer ( new_event -> calculate index to store (such that if buffer ring is full overrite the oldest one) -> )
pub struct SpmcQueue<T: EventImpl> {
    buffer: Vec<MaybeUninit<T>>,
    mask: usize,
    write_idx: CacheAligned<AtomicUsize>,
}

unsafe impl<T: EventImpl> Sync for SpmcQueue<T> {}

impl<T: EventImpl> SpmcQueue<T> {
    // power of 2 capacity allocation: For BitMask to replace modulo for cyclic idx calculations
    pub fn with_capacity_pow2(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two(), "capacity must be power of two");

        let mut buffer = Vec::with_capacity(capacity);
        buffer.resize_with(capacity, MaybeUninit::uninit);

        Self {
            buffer,
            mask: capacity - 1,
            write_idx: CacheAligned(AtomicUsize::new(0)),
        }
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    #[inline(always)]
    pub fn push(&self, value: T) {
        let idx = self.write_idx.0.fetch_add(1, Ordering::Relaxed);
        let slot = idx & self.mask;

        unsafe {
            (self.buffer.get_unchecked(slot).as_ptr() as *mut T).write(value);
        }

        std::sync::atomic::fence(Ordering::Release);
    }

    // takes self -> returns current writable idx for producer
    #[inline(always)]
    pub fn current_write(&self) -> usize {
        self.write_idx.0.load(Ordering::Acquire)
    }

    // read the stored event using a given idx:usize
    #[inline(always)]
    pub unsafe fn read_unchecked(&self, idx: usize) -> &T {
        let slot = idx & self.mask;
        unsafe { &*self.buffer.get_unchecked(slot).as_ptr() }
    }
}

pub trait EventHandler<E: EventImpl> {
    fn handle_event(&mut self, event: &E);
}

#[macro_export]
macro_rules! define_event_system {
    (
        $(
            $event:ty => {
                topic: $topic:ident,
                queue_capacity: $queue_cap:expr,
                max_handlers: $max_handlers:expr,
                emit: $emit_fn:ident,
                register: $register_fn:ident,
                poll: $poll_fn:ident
            }
        ),* $(,)?
    ) => {
        $(
            static mut $topic: Option<$crate::event_manager::EventTopic<$event, $queue_cap, $max_handlers>> = None;
        )*

        pub struct EventSystem;

        impl EventSystem {
            #[allow(static_mut_refs)]
            pub fn init() {
                unsafe {
                    $(
                        if $topic.is_none() {
                            $topic = Some($crate::event_manager::EventTopic::new());
                        }
                    )*
                }
            }

            $(
                #[allow(static_mut_refs)]
                #[inline(always)]
                pub fn $emit_fn(event: $event) {
                    unsafe {
                        if let Some(topic) = $topic.as_ref() {
                            topic.emit(event);
                        }
                    }
                }

                #[allow(static_mut_refs)]
                pub fn $register_fn<H>(
                    handler: &mut H,
                ) -> Result<usize, $crate::event_manager::RegisterHandlerError>
                where
                    H: $crate::event_manager::EventHandler<$event> + 'static,
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
                pub fn $poll_fn() {
                    unsafe {
                        if let Some(topic) = $topic.as_mut() {
                            topic.poll();
                        }
                    }
                }
            )*

            #[inline(always)]
            pub fn poll_all() {
                $(
                    Self::$poll_fn();
                )*
            }
        }
    };
}

define_event_system! {
    crate::wl_pointer::PointerEvent => {
        topic: POINTER_TOPIC,
        queue_capacity: 64,
        max_handlers: 8,
        emit: emit_pointer,
        register: register_pointer_handler,
        poll: poll_pointer
    }
}
