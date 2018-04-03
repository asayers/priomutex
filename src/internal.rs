/*!
Internals.  Not meant for consumption.

Internals are exposed for the sake of interest only.  The usual caveats apply:

* No guarantees about API stability
* The user may need to enforce invariants
* The documentation may be inaccurate

*/

use std::cmp::Ordering;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::thread::{self, Thread};

/// A value `V` with a priority `P`
#[derive(Debug, Clone)]
pub struct PV<P,V> {
    pub p: P,
    pub v: V,
}

impl<P:PartialEq,V> PartialEq for PV<P,V> {
    fn eq(&self, other: &PV<P,V>) -> bool { other.p == self.p }
}

impl<P:Eq,V> Eq for PV<P,V> {}

impl<P:PartialOrd,V> PartialOrd for PV<P,V> {
    fn partial_cmp(&self, other: &PV<P,V>) -> Option<Ordering> { other.p.partial_cmp(&self.p) }
}

impl<P:Ord,V> Ord for PV<P,V> {
    fn cmp(&self, other: &PV<P,V>) -> Ordering { other.p.cmp(&self.p) }
}

pub fn create_tokens() -> (SleepToken, WakeToken) {
    let token = Arc::new(Token {
        thread: thread::current(),
        is_woken: AtomicBool::new(false),
    });
    (SleepToken(token.clone()), WakeToken(token))
}

#[derive(Debug)]
struct Token {
    thread: Thread,
    is_woken: AtomicBool,
}
/// Note: This is NOT `Send` or `Sync`!  (Negative traits are currently unstable...)
#[derive(Debug)]
pub struct SleepToken(Arc<Token>);
#[derive(Debug)]
pub struct WakeToken(Arc<Token>);

// unsafe impl Send for WakeToken {}
// unsafe impl Sync for WakeToken {} ?
// impl !Send for SleepToken {}
// impl !Sync for SleepToken {}

impl SleepToken {
    /// Sleep the current thread until the corresponding `WakeToken` is signalled.
    pub fn sleep(self) {
        while !self.0.is_woken.load(atomic::Ordering::SeqCst) {
            thread::park();
        }
    }
}

impl WakeToken {
    /// Prevent threads from sleeping on the corresponding `SleepToken`, waking a thread if
    /// currently doing so.
    pub fn wake(self) {
        let already_woken = self.0.is_woken.compare_and_swap(false, true, atomic::Ordering::SeqCst);
        assert!(!already_woken, "this token was signalled twice!");
        self.0.thread.unpark();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_tokens() {
        let (sleep_token, wake_token) = create_tokens();
        wake_token.wake();
        sleep_token.sleep();
        println!("woke!");
    }
}
