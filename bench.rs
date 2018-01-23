use super::*;
use std::thread;
use std::sync::mpsc;
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

#[test]
fn test_thread_pool() {
    let mutex = Mutex::new((Instant::now(), vec![]));
    let mut tids = vec![];
    for n in 1..4 {
        let mutex = mutex.clone();
        tids.push(thread::spawn(move||{
            for i in 0..(10 * n) {
                let ts = {
                    let mut x = mutex.lock(n);
                    let d = x.0.elapsed();
                    x.1.push(d);
                    thread::sleep(Duration::from_millis(3));
                    let ts = Instant::now();
                    x.0 = ts;
                    ts
                };
                println!("thread {}, iter {:>2}: releasing took {:>5} ns", n, i, ts.elapsed().subsec_nanos());
                thread::sleep(Duration::from_millis(5));
            }
        }));
    }
    thread::sleep(Duration::from_millis(10));

    // Let's go!
    // guard.inner.0 = Instant::now();
    // guard.release();
    // for i in 0..30 {
    //     let mut guard = mutex.lock(9).unwrap();
    //     guard.inner.1.push(guard.inner.0.elapsed());
    //     thread::sleep(Duration::from_millis(3));
    //     let ts = Instant::now();
    //     guard.inner.0 = ts;
    //     guard.release();
    //     println!("thread 9, iter {:>2}: releasing took {:>5} ns", i, ts.elapsed().subsec_nanos());
    // }
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
