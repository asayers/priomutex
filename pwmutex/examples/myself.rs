extern crate pwmutex;
extern crate rand;

use pwmutex::*;
use rand::{Rng, thread_rng};
use std::sync::Arc;
use std::thread;
use std::time::*;

fn main() {
    let mutex = Arc::new(Mutex::new(Instant::now()));
    let mut tids = vec![];
    for n in 1..5 {
        let mutex = mutex.clone();
        tids.push(thread::spawn(move||{
            let mut rng = thread_rng();
            for _ in 0..10 {
                let ts1 = Instant::now();
                let mut x = loop {
                    if let Some(x) = mutex.try_lock_pw(thread::current().id()) { break x }
                    else { thread::yield_now(); }
                };
                println!("thread {}: LOCK    {:>5} ns", n, ts1.elapsed().subsec_nanos());
                thread::sleep(Duration::from_millis(rng.gen::<u64>() % 32));
                let ts = Instant::now();
                x.release_protected(thread::current().id());
                println!("thread {}: RELEASE {:>5} ns", n, ts.elapsed().subsec_nanos());
                thread::sleep(Duration::from_millis(rng.gen::<u64>() % 32));
            }
        }));
    }
    thread::sleep(Duration::from_millis(1000));
}
