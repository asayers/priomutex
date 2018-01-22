use std::ops::{Deref, DerefMut};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// A really simple mutex with no blocking support.  Threads must spin while waiting to lock.
pub struct Mutex<T> {
    data: UnsafeCell<T>,
    is_free: AtomicBool,
}

impl<T> Mutex<T> {
    pub fn new(data: T) -> Mutex<T> {
        Mutex {
            data: UnsafeCell::new(data),
            is_free: AtomicBool::new(true),
        }
    }

    // Make sure you call `release` before dropping the guard, or else you'll get a deadlock.
    pub fn try_lock(&self) -> Option<MutexGuard<T>> {
        if self.is_free.swap(false, Ordering::SeqCst) {
            Some(MutexGuard::new(self))
        } else {
            None
        }
    }
}

unsafe impl<T: Send> Send for Mutex<T> { }
unsafe impl<T: Send> Sync for Mutex<T> { }

pub struct MutexGuard<'a, T: 'a> {
    __lock: &'a Mutex<T>
}

impl<'a, T> MutexGuard<'a, T> {
    fn new(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
        MutexGuard {
            __lock: mutex,
        }
    }

    // You *must not* dereference this guard after calling `release`!
    pub unsafe fn release(&mut self) {
        self.__lock.is_free.store(true, Ordering::SeqCst);
    }
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.__lock.data.get() }
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.__lock.data.get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test() {
        let h = Arc::new(Mutex::new(vec![]));
        let mut tids = vec![];
        for i in 0..3 {
            let h = h.clone();
            tids.push(thread::spawn(move|| {
                loop {
                    if let Some(mut x) = h.try_lock() {
                        x.push(i);
                        thread::sleep_ms(1);
                        unsafe { x.release(); }
                        break;
                    }
                }
            }));
        }
        for tid in tids { tid.join().unwrap(); }
        println!("{:?}", *h.try_lock().unwrap());
    }
}
