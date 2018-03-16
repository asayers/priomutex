/*!
A mutex where waiting threads to specify a priority.

The API is very similar to `std::sync::Mutex`.  The key difference, of course, is that `lock` takes
a priority.  If multiple threads are waiting for the mutex when it's freed, the one which gave the
highest priorty will recieve it.

```
# use std::time::Duration;
# use std::thread;
# use std::sync::Arc;
use priomutex::simple::Mutex;

let mutex = Arc::new(Mutex::new(0));
for n in 0..3 {
    let mutex = mutex.clone();
    thread::spawn(move|| {
        loop {
            {
                let mut data = mutex.lock(0).unwrap();
                *data += 1;
            }
            thread::sleep(Duration::from_millis(1000));
        }
    });
}
```
*/

extern crate fnv;

mod simple; pub use simple::*;
pub mod spin_one;
pub mod internal;
