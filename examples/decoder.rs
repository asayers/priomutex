extern crate priomutex;
extern crate env_logger;

use priomutex::*;
use std::io::{stdin, BufRead};
use std::mem;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::*;

fn main() {
    env_logger::init();
    let (tx, rx) = mpsc::channel();
    let mutex = Arc::new(Mutex::new((rx, Instant::now())));
    for n in 0..3 {
        let mutex = mutex.clone();
        thread::spawn(move|| {
            loop {
                println!("thread {:>3}: WAIT", n);
                let mut rx = mutex.lock(0).unwrap();
                let dur_lock = rx.1.elapsed().subsec_nanos();
                println!("thread {:>3}: LOCK ({} ns)", n, dur_lock);
                rx.0.recv().unwrap();
                let ts = Instant::now();
                rx.1 = ts.clone();
                mem::drop(rx);
                let dur_release = ts.elapsed().subsec_nanos();
                println!("thread {:>3}: RELEASE ({} ns)", n, dur_release);
                thread::sleep(Duration::from_millis(1000));
            }
        });
    }
    thread::spawn(move|| {
        loop {
                println!("thread EMG: WAIT");
                let mut rx = mutex.lock(1).unwrap();
                let dur_lock = rx.1.elapsed().subsec_nanos();
                println!("thread EMG: LOCK ({} ns)", dur_lock);
                rx.0.recv().unwrap();
                let ts = Instant::now();
                rx.1 = ts.clone();
                mem::drop(rx);
                let dur_release = ts.elapsed().subsec_nanos();
                println!("thread EMG: RELEASE ({} ns)", dur_release);
        }
    });
    thread::sleep(Duration::from_millis(10));
    let stdin = stdin();
    for _ in stdin.lock().lines() {
        tx.send(()).unwrap();
    }
}
