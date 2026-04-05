use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(align(64))]
struct CacheAligned<T>(T);

// Marker trait used to constrain events stored in the SPMC queue.
pub trait EventImpl: Copy + Send + Sync + 'static {}

// Single-producer, multi-consumer ring queue primitive.
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
