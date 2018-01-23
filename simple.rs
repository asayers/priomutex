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

/// An RAII guard.  Can be dereferenced to access the data protected by the mutex.  Frees the mutex
/// when dropped.
pub struct MutexGuard<'a, T: 'a> {
    __lock: &'a Mutex<T>,
    __is_valid: bool,
}

impl<'a, T> MutexGuard<'a, T> {
    fn new(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
        MutexGuard {
            __lock: mutex,
            __is_valid: true,
        }
    }

    /// Invalidate the guard and relesase the lock.
    ///
    /// **It is not necessary to call this function yourself**, since it will be run automatically
    /// when the guard goes out of scope.  This function is useful if, for some reason, you need to
    /// free the lock without dropping the guard.
    ///
    /// Calling `release` multiple times is safe, but attempting to dereference a guard after
    /// calling `release` on it will result in a panic!
    pub fn release(&mut self) {
        if self.__is_valid {
            self.__is_valid = false;
            self.__lock.is_free.store(true, Ordering::SeqCst);
        }
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.release();
    }
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        assert!(self.__is_valid);
        unsafe { &*self.__lock.data.get() }
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        assert!(self.__is_valid);
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
