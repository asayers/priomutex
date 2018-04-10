extern crate priomutex;
extern crate rand;

use priomutex::spin_one::Mutex;
use rand::*;
use std::sync::Arc;
use std::thread;
use std::time::*;

fn main() {
    let mutex = Arc::new(Mutex::new(Instant::now()));
    let mut tids = vec![];
    for n in 1..10 {
        let mutex = mutex.clone();
        tids.push(thread::spawn(move||{
            let mut rng = thread_rng();
            for i in 0..10 {
                let ts = {
                    let mut x = mutex.lock(n).unwrap();
                    println!("thread {}: LOCK    #{:<2} {:>5} ns", n, i, x.elapsed().subsec_nanos());
                    thread::sleep(Duration::from_millis(rng.gen::<u64>() % 32));
                    let ts = Instant::now();
                    *x = ts;
                    ts
                };
                println!("thread {}: RELEASE #{:<2} {:>5} ns", n, i, ts.elapsed().subsec_nanos());
                thread::sleep(Duration::from_millis(rng.gen::<u64>() % 32));
            }
        }));
    }
    for t in tids {
        t.join().unwrap();
    }
    let guard = mutex.lock(0).unwrap();
    println!("{:?}", *guard);
}
