extern crate priomutex;
extern crate rand;

use priomutex::Mutex;
use rand::*;
use std::mem;
use std::sync::Arc;
use std::thread;
use std::time::*;

#[test]
fn test_in_order_locking() {
    const N: usize = 10;
    let mut rng = thread_rng();
    let mut prios: Vec<usize> = (0..N).map(|_| rng.gen::<usize>()).collect();

    let mutex = Arc::new(Mutex::new(Vec::new()));
    let guard = mutex.lock(0).unwrap();

    let mut tids = Vec::new();
    for &prio in prios.iter() {
        let mutex = mutex.clone();
        tids.push(thread::spawn(move || {
            let mut data = mutex.lock(prio).unwrap(); // wait on the mutex
            data.push(prio);                      // push priority onto the list
        }));
    }

    // Give the threads time to spawn and wait on the mutex
    thread::sleep(Duration::from_millis(1000));
    mem::drop(guard);             // go go go!

    for t in tids { t.join().unwrap(); }   // wait until they've all modified the mutex

    // Check that the threads were woken in priority order
    prios.sort();
    let prios2 = mutex.lock(0).unwrap();
    assert_eq!(prios, *prios2);
}

#[test]
fn test_each_takes_once() {
    let h = Arc::new(Mutex::new(vec![]));
    let mut tids = vec![];
    for i in 0..5 {
        let h = h.clone();
        tids.push(thread::spawn(move|| {
            let mut x = h.lock(10-i).unwrap();
            thread::sleep(Duration::from_millis(10));
            x.push(i);
        }));
    }
    for tid in tids { tid.join().unwrap(); }
}

// Check that the releasing thread doesn't have an unfair advantage in re-taking
#[test]
fn test_no_unfair_advantage() {
    let m1 = Arc::new(Mutex::new(0));
    let m2 = m1.clone();
    {
        let mut g = m1.lock(0).unwrap();   // thread 1 takes the lock first
        thread::spawn(move|| {       // thread 2 simply:
            let mut g = m2.lock(0).unwrap();  // waits for the lock...
            thread::sleep(Duration::from_millis(1000));  // and holds it forever (effectively)
            *g += 1;
        });
        thread::sleep(Duration::from_millis(500)); // let thread 2 fully go to sleep
        *g += 1;
    } // now release... and immediately try to re-acquire
    for _ in 0..100 {
        if m1.try_lock().is_ok() {
            panic!("try_lock succeeded when there was a thread waiting!");
        }
    }
}

#[test]
fn test_single_thread() {
    let mutex = Arc::new(Mutex::new(0u8));
    let x = mutex.lock(0).unwrap();
    mem::drop(x);
    let x = mutex.lock(1).unwrap();
    mem::drop(x);
    let x = mutex.lock(2).unwrap();
    mem::drop(x);
    let x = mutex.lock(3).unwrap();
    mem::drop(x);
    let x = mutex.lock(4).unwrap();
    mem::drop(x);
}

/*
#[test]
fn test_thread_pool_free() {
    let mutex = Arc::new(Mutex::new(0u8));
    let h1 = mutex.clone();
    let h2 = mutex.clone();
    thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::spawn(move|| { let guard = h2.lock(1).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_thread_pool_neq() {
    let mutex = Arc::new(Mutex::new(0u8));
    let h1 = mutex.clone();
    let h2 = mutex.clone();
    thread::spawn(move|| { let guard = h2.lock(1).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::sleep(Duration::from_millis(10));
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_thread_pool_eq() {
    let mutex = Arc::new(Mutex::new(0u8));
    let h1 = mutex.clone();
    let h2 = mutex.clone();
    thread::spawn(move|| { let guard = h2.lock(0).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::sleep(Duration::from_millis(100));
}

*/
