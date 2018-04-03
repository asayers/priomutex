use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::thread::{self, Thread};

/// Create a linked pair of tokens.
///
/// Note that the `SleepToken` is valid for the current thread only.  Don't send it to another
/// thread!
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

/// A token for putting the current thread to sleep.
///
/// Note: This is NOT `Send` or `Sync`!  (Negative traits are currently unstable...)
#[derive(Debug)]
pub struct SleepToken(Arc<Token>);

/// A token for waking a thread
#[derive(Debug)]
pub struct WakeToken(Arc<Token>);

// unsafe impl Send for WakeToken {}
// unsafe impl Sync for WakeToken {} ?
// impl !Send for SleepToken {}
// impl !Sync for SleepToken {}

impl SleepToken {
    /// Sleep the current thread until the corresponding `WakeToken` is signalled.  If the
    /// `WakeToken` has *already* been signalled, this function returns immediately.
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
