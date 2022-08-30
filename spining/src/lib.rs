//! example code in TAOMP ch7 for spining lock and contention

#![feature(atomic_mut_ptr)]
#![feature(negative_impls)]
use loom::sync::atomic::AtomicBool;
use loom::sync::atomic::AtomicPtr;
use loom::sync::atomic::Ordering;
use loom::cell::UnsafeCell;
use loom::cell::Cell;
use loom::thread;
use loom::sync::atomic;
use core::ops::Deref;
use core::ops::DerefMut;
use core::ptr::NonNull;
use core::ptr;


/// Test-And-Set Spin Lock impl
pub struct TASLock<T: ?Sized> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Sync + Send> Send for TASLock<T> {}
unsafe impl<T: ?Sized + Sync + Send> Sync for TASLock<T> {}

/// Guard RAII for TASLock
pub struct TASLockGuard<'a, T: ?Sized + 'a> {
    lock: &'a TASLock<T>,
}

impl<'mutex, T: ?Sized> TASLockGuard<'mutex, T> {
    fn new(lock: &'mutex TASLock<T>) -> Self {
        Self {
            lock
        }
    }
}

impl<T: ?Sized> !Send for TASLockGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for TASLockGuard<'_, T>{}

impl<T> TASLock<T> {
    
    pub fn new(t: T) -> TASLock<T> {
        TASLock {
            data: UnsafeCell::new(t),
            locked: AtomicBool::new(false),
        }
    }
}

impl<T: ?Sized> TASLock<T> {
    
    pub fn lock(&self) -> TASLockGuard<'_, T> {
       while self.locked.fetch_or(true, Ordering::Acquire){}
        TASLockGuard::new(self) 
    }

    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release)
    }
}

impl<T: ?Sized> Deref for TASLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            &*{
                let tracked_ptr = self.lock.data.get();
                tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}


impl<T: ?Sized> DerefMut for TASLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            &mut*{
                let tracked_ptr = self.lock.data.get_mut();
                tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}

impl<T: ?Sized> Drop for TASLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.unlock()        
    }
}

/// Test-Test-And-Set Spin Lock
pub struct TTASLock<T: ?Sized> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Sync + Send> Send for TTASLock<T> {}
unsafe impl<T: ?Sized + Sync + Send> Sync for TTASLock<T> {}

impl<T> TTASLock<T> {
    pub  fn new(t: T) -> Self {
        Self {
            data: UnsafeCell::new(t),
            locked: AtomicBool::new(false),
        }
    }
}

pub struct TTASLockGuard<'a, T: ?Sized + 'a> {
    lock: &'a TTASLock<T>,
}

impl<T: ?Sized> !Send for TTASLockGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for TTASLockGuard<'_, T> {}

impl<'mutex, T: ?Sized> TTASLockGuard<'mutex, T> {
    fn new(lock: &'mutex TTASLock<T>) -> Self {
        Self {
            lock
        }
    }
}

impl<T: ?Sized> TTASLock<T> {
    
    pub fn lock(&self) -> TTASLockGuard<'_, T> {
        loop {
            // FIXME: maybe load is not sound if there some approach voltila read ?
            while self.locked.load(Ordering::Acquire) {}
            if !self.locked.swap(true, Ordering::Acquire) {
                break;
            }
        }
        TTASLockGuard::new(self)
    }

    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release)
    }
}

impl<T: ?Sized> Deref for TTASLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            &*{
               let tracked_ptr = self.lock.data.get();
               tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}


impl<T: ?Sized> DerefMut for TTASLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            &mut*{
                let tracked_ptr = self.lock.data.get_mut();
                tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}

impl<T: ?Sized> Drop for TTASLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.unlock()        
    }
}



/// Backoff primitives
/// adapt from crossbeam with const generics 
pub struct Backoff<const SPIN_LIMIT: u32, const YIELD_LIMIT: u32> {
    step: Cell<u32>
}

impl<const SPIN_LIMIT: u32, const YIELD_LIMIT: u32> Backoff<SPIN_LIMIT, YIELD_LIMIT> {
   
    pub fn new() -> Self {
        Self {
            step: Cell::new(0)
        }
    }

    pub fn reset(&self) {
        self.step.set(0)
    }


    pub fn spin(&self) {
        for _ in 0..1<< self.step.get().min(SPIN_LIMIT) {
            atomic::spin_loop_hint();
        }
        if self.step.get() <= SPIN_LIMIT {
            self.step.set(self.step.get() + 1);
        }
    }

    pub fn backoff(&self) {
        if self.step.get() <= SPIN_LIMIT {
            for _ in 0..1 << self.step.get() {
                atomic::spin_loop_hint();
            }
        } else {
            thread::yield_now()
        }

        if self.step.get() <= YIELD_LIMIT {
            self.step.set(self.step.get() + 1)
        }
    }

    pub fn advised_park(&self) -> bool {
        self.step.get() > YIELD_LIMIT
    }
}

// TODO: yet another plain QueueLock ?


pub struct CLHNode(AtomicBool);

impl CLHNode {
    pub fn new(locked: bool) -> Self {
        Self(AtomicBool::new(locked))
    }
}

/// design for SMP arch spin lock
pub struct CLHLock<T: ?Sized> {
    // in practical implementation have to care about the Cache padding
    tail: AtomicPtr<CLHNode>,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Sync + Send> Sync for CLHLock<T> {}
unsafe impl<T: ?Sized + Sync + Send> Send for CLHLock<T> {}

impl<T> CLHLock<T> {
    pub fn new(t: T) -> Self {
        Self{
            tail: AtomicPtr::new(Box::into_raw(Box::new(CLHNode::new(false)))),
            data: UnsafeCell::new(t),
        }
    }
}

impl<T: ?Sized> CLHLock<T> {

    pub fn lock(&self) -> CLHLockGuard<'_, T> {
        let node = Box::into_raw(Box::new(CLHNode::new(true)));
        let prev = self.tail.swap(node, Ordering::Relaxed);
        unsafe {
            while (*prev).0.load(Ordering::Acquire) {}
        }
        CLHLockGuard::new(self, NonNull::new(node).unwrap())
    }

    pub fn unlock(&self, token: NonNull<CLHNode>) {
       unsafe {
            (*token.as_ref()).0.store(false, Ordering::Release)
       } 
    }
}

pub struct CLHLockGuard<'a, T: ?Sized + 'a> {
    lock: &'a CLHLock<T>,
    token: NonNull<CLHNode>,
}

impl<T: ?Sized> !Send for CLHLockGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for CLHLockGuard<'_, T> {}


impl<'mutex, T: ?Sized> CLHLockGuard<'mutex, T> {
    fn new(lock: &'mutex CLHLock<T>, token: NonNull<CLHNode>) -> Self {
        Self {
            lock,
            token
        }
    }
}

impl<T: ?Sized> Deref for CLHLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            &*{
               let tracked_ptr = self.lock.data.get();
               tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}


impl<T: ?Sized> DerefMut for CLHLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            &mut*{
                let tracked_ptr = self.lock.data.get_mut();
                tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}

impl<T: ?Sized> Drop for CLHLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.unlock(self.token)        
    }
}


pub struct MCSNode {
    locked: bool,
    next: *mut MCSNode,
}

impl MCSNode {
    pub fn new(locked: bool) -> MCSNode {
        Self {
            locked,
            next: ptr::null_mut(),
        }
    }
}


/// Design for NUMA arch spin lock
pub struct MCSLock<T: ?Sized> {
    tail: AtomicPtr<MCSNode>,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send + Sync> Send for MCSLock<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for MCSLock<T> {}

impl<T> MCSLock<T> {
    pub fn new(t: T) -> Self {
        Self {
            tail: AtomicPtr::new(ptr::null_mut()),
            data: UnsafeCell::new(t),
        }
    }
}

impl<T: ?Sized> MCSLock<T> {
    
    pub fn lock(&self) -> MCSLockGuard<'_, T> {
        let node = Box::into_raw(Box::new(MCSNode::new(false)));
        let pred = self.tail.swap(node, Ordering::Acquire);
        if !pred.is_null() {
            unsafe { 
                (&mut *node).locked = true;
                (&mut *pred).next = node;
                while (&*node).locked {}
            }
        }
        MCSLockGuard::new(self, NonNull::new(node).unwrap())
    }

    pub fn unlock(&self, mut token: NonNull<MCSNode>) {
        unsafe { 
             if token.as_ref().next.is_null() {
                match self.tail.compare_exchange_weak(
                    token.as_ptr(),
                    ptr::null_mut(),
                    Ordering::Release,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return,
                    Err(_) => {},
                }

                while token.as_ref().next.is_null() {}
             }
             (&mut *token.as_mut().next).locked = false;
            (&mut *token.as_ptr()).next = ptr::null_mut();
        }
    }
}

pub struct MCSLockGuard<'a, T: ?Sized + 'a> {
    lock: &'a MCSLock<T>,
    token: NonNull<MCSNode>,
}

impl<T: ?Sized> !Send for MCSLockGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for MCSLockGuard<'_, T> {}

impl<'mutex, T: ?Sized> MCSLockGuard<'mutex, T> {
    fn new(lock: &'mutex MCSLock<T>, token: NonNull<MCSNode>) -> Self {
        Self {
            lock,
            token,
        }
    }
}

impl<T: ?Sized> Deref for MCSLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            &*{
               let tracked_ptr = self.lock.data.get();
               tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}


impl<T: ?Sized> DerefMut for MCSLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            &mut*{
                let tracked_ptr = self.lock.data.get_mut();
                tracked_ptr.with(|real_ptr| real_ptr)
            }
        }
    }
}

impl<T: ?Sized> Drop for MCSLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.unlock(self.token)        
    }
}


/// Plain Timeout Lock 
pub struct TOLock;

#[cfg(test)]
mod tests {
    use super::*;
    // TODO: need test in loom
}
