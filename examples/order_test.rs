extern crate priomutex;
extern crate rand;

use priomutex::Mutex;
use rand::{Rng, thread_rng};
use std::mem;
use std::sync::Arc;
use std::thread;

fn main() {
    let mutex = Arc::new(Mutex::new(()));
    let guard = mutex.lock(0).unwrap();  // take the lock
    let mut tids = vec![];
    for _ in 0..20 {
        let mutex = mutex.clone();
        tids.push(thread::spawn(move|| {
            let p = thread_rng().gen::<usize>(); // generate a random priority
            println!("Blocking with priority {}", p);
            let guard = mutex.lock(p).unwrap();  // block until we take the lock
            println!("Took the lock with priority {}", p);
            assert_eq!(*guard, ());
            // lock is released here
        }));
    }
    thread::sleep_ms(100);  // wait for the threads to spawn
    mem::drop(guard);  // go go go!
    for tid in tids {
        tid.join().unwrap();
    }
}
