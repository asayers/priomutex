extern crate priomutex;

use priomutex::*;
use std::io::{stdin, BufRead};
use std::mem;
use std::sync::mpsc;
use std::thread;
use std::time::*;

fn main() {
    let (tx, rx) = mpsc::channel();
    let mutex = Mutex::new(rx);
    for n in 0..3 {
        let mutex = mutex.clone();
        thread::spawn(move|| {
            loop {
                println!("thread {:>3}: WAIT", n);
                let rx = mutex.lock(0);
                println!("thread {:>3}: LOCK", n);
                rx.recv().unwrap();
                let ts = Instant::now();
                mem::drop(rx);
                println!("thread {:>3}: RELEASE ({} ns)", n, ts.elapsed().subsec_nanos());
                thread::sleep(Duration::from_millis(1000));
            }
        });
    }
    thread::spawn(move|| {
        loop {
                println!("thread EMG: WAIT");
                let rx = mutex.lock(1);
                println!("thread EMG: LOCK");
                rx.recv().unwrap();
                let ts = Instant::now();
                mem::drop(rx);
                println!("thread EMG: RELEASE ({} ns)", ts.elapsed().subsec_nanos());
        }
    });
    thread::sleep(Duration::from_millis(10));
    let stdin = stdin();
    for _ in stdin.lock().lines() {
        tx.send(()).unwrap();
    }
}
