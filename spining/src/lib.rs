#![feature(atomic_mut_ptr)]

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

pub trait SpinLock {
    fn lock(&self);
    fn unlock(&self);
}


/// Test-And-Set Spin Lock impl 
pub struct TASLock(AtomicBool);

unsafe impl Sync for TASLock{}

impl TASLock {
    #[allow(dead_code)]
    fn new() -> Self {
        Self(AtomicBool::new(false))
    }
}

impl SpinLock for TASLock {

    fn lock(&self) {
        while self.0.fetch_or(true, Ordering::Relaxed){}
    }

    fn unlock(&self) {
        self.0.store(false, Ordering::Relaxed)
    }
}


/// Test-Test-And-Set Spin Lock
pub struct TTASLock(AtomicBool);

unsafe impl Sync for TTASLock{}

impl TTASLock {
    #[allow(dead_code)]
    fn new() -> Self {
        Self(AtomicBool::new(false))
    }
}

impl SpinLock for TTASLock {
    
    fn lock(&self) {
        loop {
            // FIXME: maybe not really sound in rust
            unsafe {
                while *self.0.as_mut_ptr() {}
                // TODO: if we need a fence to protect this to prevent re-order after this 
                if !self.0.fetch_or(true, Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    fn unlock(&self) {
        self.0.store(false, Ordering::Relaxed)
    }
}

// TODO: impl them 

pub struct QueueLock;

pub struct CLHLock;

pub struct MCSLock;

pub struct TOLock;



#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
