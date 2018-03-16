/*!
A mutex where waiting threads to specify a priority.

The API is very similar to `std::sync::Mutex`.  The key difference, of course, is that `lock` takes
a priority.  If multiple threads are waiting for the mutex when it's freed, the one which gave the
highest priorty will recieve it.

The other difference is that `std::sync::Mutex` implements `Sync` but not `Clone`, whereas
`priomutex::Mutex` implements `Clone` but not `Sync`.  In practice this means (1) that you don't
need to wrap a priomutex in an `Arc`, and (2) that we can't implement `into_inner` and `get_mut`.

```
# extern crate rand;
# extern crate priomutex;
# fn main() {
use priomutex::simple::Mutex;
use rand::{Rng, thread_rng};
use std::mem;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const N: usize = 10;

let m = Arc::new(Mutex::new(Vec::new()));
let guard = m.lock(0).unwrap();

let mut tids = Vec::new();
for _ in 0..N {
    let m = m.clone();
    tids.push(thread::spawn(move || {
        let mut rng = thread_rng();
        let prio = rng.gen::<usize>();               // generate a random priority
        let mut data = m.lock(prio).unwrap();     // wait on the mutex
        data.push(prio);                             // push priority onto the list
    }));
}

// Give the threads time to spawn and wait on the mutex
thread::sleep(Duration::from_millis(10));

mem::drop(guard);             // go go go!
for t in tids { t.join(); }   // wait until they've all modified the mutex

// Check that every thread pushed an element
let d1 = m.lock(0).unwrap();
assert_eq!(d1.len(), N);

// Check that the threads were woken in priority order
let mut d2 = d1.clone(); d2.sort();
assert_eq!(*d1, d2);
# }
```
*/

extern crate fnv;

pub mod simple;
pub mod spin_one;
mod common;
