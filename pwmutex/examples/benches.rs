extern crate easybench;
extern crate pwmutex;

use easybench::*;
use pwmutex::*;
use std::sync::Arc;
use std::thread;
use std::time::*;

fn main() {
    bench_lock_release();
    bench_lock_release_protected();
    bench_lock_failure();
    bench_lock_release_contested();
    bench_ring();
}

fn bench_lock_release() {
    println!("bench_lock_release()");
    let m = Arc::new(Mutex::new(0));
    println!("{}", bench_env(m, |m| { let mut g = m.try_lock_pw("foo").unwrap(); g.release() }));
}

fn bench_lock_release_protected() {
    println!("bench_lock_release_protected()");
    let m = Arc::new(Mutex::new(0));
    println!("{}", bench_env(m, |m| { let mut g = m.try_lock_pw("foo").unwrap(); g.release_protected("foo") }));
}

fn bench_lock_failure() {
    println!("bench_lock_failure()");
    let m = Arc::new(Mutex::new(0));
    let mut g = m.try_lock_pw("foo").unwrap();
    println!("{}", bench_env(m.clone(), |m| { let g = m.try_lock_pw("foo"); assert!(g.is_none()) }));
    g.release();
}

fn bench_lock_release_contested() {
    println!("bench_lock_release_contested()");
    let m = Arc::new(Mutex::new(0));
    let mut tids = vec![];
    for _ in 0..3 {
        let m1 = m.clone();
        tids.push(::std::thread::spawn(move|| {
            println!("{}", bench_env(m1, |m2| {
                let g = m2.try_lock_pw("foo");
                if let Some(mut g) = g { g.release(); }
            }));
        }));
    }
    for t in tids { t.join().unwrap(); }
}

// 1 => ~25 ns
// 2 => ~175 ns
// 3 => ~225 ns
// 4 => ~250 ns
// 5 => ~300 ns
fn bench_ring() {
    println!("bench_ring()");
    bench_ring_(1);
    bench_ring_(2);
    bench_ring_(3);
    bench_ring_(4);
    bench_ring_(5);
}

fn bench_ring_(threads: usize) {
    let m = Arc::new(Mutex::new(0));
    let mut g = m.try_lock_pw(0).unwrap();
    let mut tids = vec![];
    const ITERS: usize = 100_000;
    for i in 0..threads {
        let m1 = m.clone();
        tids.push(::std::thread::spawn(move|| {
            for _ in 0..ITERS {
                let mut g = loop {
                    if let Some(x) = m1.try_lock_pw(i) { break x }
                    else { thread::yield_now(); }
                };
                *g += 1;
                g.release_protected((i + 1) % threads);
            }
        }));
    }
    thread::sleep(Duration::from_millis(1));
    let ts = Instant::now();
    g.release();
    for t in tids { t.join().unwrap(); }
    println!("    {} threads: {} ns", threads,
             ts.elapsed().subsec_nanos() as usize / (threads * ITERS));
}
