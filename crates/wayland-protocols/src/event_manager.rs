use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::wl_pointer::Pointer;

#[repr(align(64))]
struct CacheAligned<T>(T);

// Currently just a marker trait to abstract Events
pub trait EventImpl: Copy + Send + Sync + 'static {}

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
    fn handle_event(&mut self, event: E) {}
}

#[macro_export]
macro_rules! event_handler {
    (
        $name:ident {
            $(
                $event:ty => {
                    queue: $queue:ident,
                    handlers: [ $($handler:path),* $(,)? ]
                }
            ),* $(,)?
        }
    ) => {
        pub struct $name {
            $(
                $queue: usize,
            )*
        }

        impl $name {
            pub fn new() -> Self {
                Self {
                    $(
                        $queue: 0,
                    )*
                }
            }

            #[inline(always)]
            #[allow(static_mut_refs)]
            pub fn poll(&mut self) {
                unsafe {
                    $(
                        {
                            let q = $queue.as_ref().unwrap();
                            let write = q.current_write();
                            let capacity = q.mask + 1;

                            if write - self.$queue > capacity {
                                self.$queue = write - capacity;
                            }

                            while self.$queue < write {
                                let event = q.read_unchecked(self.$queue);

                                $(
                                    $handler(event);
                                )*

                                self.$queue += 1;
                            }
                        }
                    )*
                }
            }
        }
    };
}

// static mut KEY_QUEUE: Option<SpmcQueue<KeyPressed>> = None;

// pub struct EventSystem {
// }

// impl EventSystem {
//     pub fn init() {
//         unsafe {
//             KEY_QUEUE = Some(SpmcQueue::with_capacity_pow2(64));
//         }
//     }

//     #[allow(static_mut_refs)]
//     pub fn emit(event: KeyPressed) {
//         unsafe {
//             KEY_QUEUE.as_ref().unwrap().push(event);
//         }
//     }
// }

#[macro_export]
macro_rules! event_queues {
    (
        $(
            $event:ty => {
                queue: $queue:ident,
                capacity: $cap:expr
            }
        ),* $(,)?
    ) => {
        $(
            static mut $queue: Option<SpmcQueue<$event>> = None;
        )*

        pub struct EventSystem;

        impl EventSystem {
            pub fn init() {
                unsafe {
                    $(
                        $queue = Some(SpmcQueue::with_capacity_pow2($cap));
                    )*
                }
            }

            $(
                #[inline(always)]
                #[allow(static_mut_refs)]
                pub fn emit(event: $event) {
                    unsafe {
                        $queue.as_ref().unwrap().push(event);
                    }
                }
            )*
        }
    };
}

// mod tests {
//     use crate::event_manager::{EventSystem, Logger, PointerEvent};

//     #[test]
//     fn test_event_manager_macro() {
//         EventSystem::init();

//         let mut logger = Logger::new();

//         EventSystem::emit(PointerEvent::OnClick);

//         logger.poll();

//         assert!(true);
//     }
// }

use crate::wl_pointer::{PointerEvent, log_pointer};
event_queues! {
    PointerEvent => {
        queue: POINTER_QUEUE,
        capacity: 64
    }
}

event_handler! {
    Logger {
        KeyPressed => {
            queue: POINTER_QUEUE,
            handlers: [log_pointer]
        }
    }
}
