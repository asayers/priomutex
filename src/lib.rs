/*!
A mutex where waiting threads specify a priority.

Exactly like `std::sync::Mutex`, except that `lock` takes an integer priority (0 is high).  When
the mutex is released, the thread which gave the highest priority will take the lock.

**Status**:  As far as I can tell it's correct, although not particularly fast (releasing the lock
takes 3-4μs on my machine).

```
# extern crate rand;
# extern crate priomutex;
# fn main() {
use priomutex::Mutex;
use rand::{Rng, thread_rng};
use std::mem;
use std::sync::Arc;
use std::thread;

let mutex = Arc::new(Mutex::new(()));
let guard = mutex.lock(0).unwrap();  // take the lock

for _ in 0..10 {
    let mutex = mutex.clone();
    thread::spawn(move|| {
        let p = thread_rng().gen::<usize>(); // generate a random priority
        let guard = mutex.lock(p).unwrap();  // block until we take the lock
        println!("Took the lock: {}", p);
        // lock is released here
    });
}

thread::sleep_ms(100);  // give the threads a chance to spawn
mem::drop(guard);       // go go go!

// At this point you should see the lock being taken in priority-order
# }
```
*/

extern crate fnv;

mod simple; pub use simple::*;
// pub mod spin_one;   // contains a deadlock
pub mod internal;
