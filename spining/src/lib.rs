#![feature(atomic_mut_ptr)]
#![feature(local_key_cell_methods)]
#![feature(sync_unsafe_cell)]
use std::borrow::Borrow;
use std::cell::RefCell;
use std::cell::UnsafeCell;
use std::cell::SyncUnsafeCell;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;
use std::thread_local;

pub trait SpinLock {
    fn lock(&self);
    fn unlock(&self);
}

/// Test-And-Set Spin Lock impl
pub struct TASLock(AtomicBool);

unsafe impl Sync for TASLock {}

impl TASLock {
    #[allow(dead_code)]
    fn new() -> Self {
        Self(AtomicBool::new(false))
    }
}

impl SpinLock for TASLock {
    fn lock(&self) {
        while self.0.fetch_or(true, Ordering::Acquire) {}
    }

    fn unlock(&self) {
        self.0.store(false, Ordering::Release)
    }
}

/// Test-Test-And-Set Spin Lock
pub struct TTASLock(AtomicBool);

unsafe impl Sync for TTASLock {}

impl TTASLock {
    #[allow(dead_code)]
    fn new() -> Self {
        Self(AtomicBool::new(false))
    }
}

impl SpinLock for TTASLock {
    fn lock(&self) {
        loop {
            unsafe {
                while *self.0.as_mut_ptr() {}
                if !self.0.fetch_or(true, Ordering::Acquire) {
                    break;
                }
            }
        }
    }

    fn unlock(&self) {
        self.0.store(false, Ordering::Release)
    }
}

/// n-threads queue lock FIFO
pub struct QueueLock {
    flag: SyncUnsafeCell<Vec<bool>>,
    size: usize,
    tail: AtomicUsize,
}

unsafe impl Sync for QueueLock {}

impl QueueLock {
    thread_local! {
        static SlotIndex: RefCell<usize> = RefCell::new(0);
    }

    pub fn new(capcity: usize) -> Self {
        Self {
            flag: SyncUnsafeCell::new(Vec::with_capacity(capcity)),
            size: capcity,
            tail: AtomicUsize::new(0),
        }
    }
}

impl SpinLock for QueueLock {
    fn lock(&self) {
        let slot = self.tail.fetch_add(1, Ordering::Relaxed) % self.size;
        QueueLock::SlotIndex.set(slot);
        unsafe { while !(&*self.flag.get())[slot] {} }
    }

    fn unlock(&self) {
        let slot = QueueLock::SlotIndex.take();
        unsafe {
            (&mut *self.flag.get())[slot] = false;
            (&mut *self.flag.get())[(slot + 1) % self.size] = true;
        }
    }
}

struct QNode(bool);

/// Design for SMP arch spin lock
pub struct CLHLock {
    tail: AtomicPtr<QNode>,
    // Use the Box to allocate on Heap instead of Stack 
    // leak it to keep the pointer
}

impl CLHLock {
    thread_local! {
        static PRED_NODE: RefCell<NonNull<QNode>> = RefCell::new(NonNull::dangling());
        static LOCAL_NODE: UnsafeCell<QNode> = UnsafeCell::new(QNode(false));
    }
}

impl SpinLock for CLHLock {
    
    fn lock(&self) {
        unsafe {
            CLHLock::LOCAL_NODE.with(|x| {
                let mut qnode = x.get();
                (&mut *qnode).0 = true;
                // maybe not realy correct 
                // figure out the semantics of get_and_set
                let pred = self.tail.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some(qnode));

            })
        }

    }

    fn unlock(&self) {

    }
}

/// Design for NUMA arch spin lock
pub struct MCSLock;

pub struct TOLock;

