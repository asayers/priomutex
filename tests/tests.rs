extern crate priomutex;
extern crate rand;

use priomutex::*;
use rand::*;
use std::thread;
use std::time::*;

#[test]
fn test() {
    let h = Mutex::new(vec![]);
    let mut tids = vec![];
    for i in 0..5 {
        let h = h.clone();
        tids.push(thread::spawn(move|| {
            let mut x = h.lock(10-i);
            thread::sleep(Duration::from_millis(10));
            x.push(i);
        }));
    }
    for tid in tids { tid.join().unwrap(); }
    println!("{:?}", *h.lock(9));
}

// Check that the releasing thread doesn't have an unfair advantage in re-taking
#[test]
fn test_no_unfair_advantage() {
    let m1 = Mutex::new(0);
    let m2 = m1.clone();
    {
        let mut g = m1.lock(0);   // thread 1 takes the lock first
        thread::spawn(move|| {       // thread 2 simply:
            let mut g = m2.lock(0);  // waits for the lock...
            thread::sleep(Duration::from_millis(1000));  // and holds it forever (effectively)
            *g += 1;
        });
        thread::sleep(Duration::from_millis(500)); // let thread 2 fully go to sleep
        *g += 1;
    } // now release... and immediately try to re-acquire
    for _ in 0..100 {
        if m1.try_lock().is_some() {
            panic!("try_lock succeeded when there was a thread waiting!");
        }
    }
}

#[test]
fn test_bench() {
    let mutex = Mutex::new(Instant::now());
    let mut tids = vec![];
    for n in 1..10 {
        let mutex = mutex.clone();
        tids.push(thread::spawn(move||{
            let mut rng = thread_rng();
            for i in 0..10 {
                let ts = {
                    let mut x = mutex.lock(n);
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
    let guard = mutex.lock(0);
    println!("{:?}", *guard);
}


/*
#[test]
fn test_thread_pool_free() {
    let mutex = Mutex::new(0u8);
    let h1 = mutex.clone();
    let h2 = mutex.clone();
    thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::spawn(move|| { let guard = h2.lock(1).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_thread_pool_neq() {
    let mutex = Mutex::new(0u8);
    let h1 = mutex.clone();
    let h2 = mutex.clone();
    thread::spawn(move|| { let guard = h2.lock(1).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::sleep(Duration::from_millis(10));
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_thread_pool_eq() {
    let mutex = Mutex::new(0u8);
    let h1 = mutex.clone();
    let h2 = mutex.clone();
    thread::spawn(move|| { let guard = h2.lock(0).unwrap(); println!("got mutex: t1"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::spawn(move|| { let guard = h1.lock(0).unwrap(); println!("got mutex: t0"); thread::sleep(Duration::from_millis(10)); mem::drop(guard); });
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_thread_pool_1() {
    let mutex = Mutex::new(0u8);
    let x = mutex.lock(0).unwrap();
    x.release();
    let x = mutex.lock(1).unwrap();
    x.release();
    let x = mutex.lock(2).unwrap();
    x.release();
    let x = mutex.lock(3).unwrap();
    x.release();
    let x = mutex.lock(4).unwrap();
    x.release();
}

*/
