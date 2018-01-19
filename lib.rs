/*!
A baton encapsulates some state which may be passed around between the threads in a thread pool.
At any given time, the baton is held by exactly one member of the pool.

We distinguish between two kinds of thread: "regular" threads, of which there may be zero or many,
and which recieve the baton with high priority; and the "emergency" thread, of which there must be
exactly one, which will recieve the baton only when none of the regular threads are available to
take it.

Each thread has a "handle" which it keeps hold of at all times.  This handle is used to take the
baton.  Don't call `tag_in` on your handle while holding the baton, or else the pool will be
deadlocked.

The emergency thread must not die or drop its handle. When it's holding the baton, it should call
`tag_out` regularly in order to attempt to pass it back to a regular thread. Whenver it isn't
holding the baton it should be blocked on a call to `tag_in`, to prevent the baton from being stuck
in the emergency thread's inbox.
*/
use std::sync::mpsc;

/// A baton encapsulates some state which may be passed around between the threads in a thread
/// pool.
pub struct Baton<T> {
    queue_rx: mpsc::Receiver<mpsc::SyncSender<Baton<T>>>,
    emergency_tx: mpsc::SyncSender<Baton<T>>,
    pub inner: T,
}

/// Allows taking the baton with high priority.
pub struct RegularHandle<T>(mpsc::Sender<mpsc::SyncSender<Baton<T>>>);
impl<T> Clone for RegularHandle<T> {
    fn clone(&self) -> Self {
        RegularHandle(self.0.clone())
    }
}

/// Allows taking the baton with low priority.
pub struct EmergencyHandle<T>(mpsc::Receiver<Baton<T>>);

unsafe impl<T: Send> Send for RegularHandle<T> {}
unsafe impl<T: Send> Send for EmergencyHandle<T> {}

impl<T> Baton<T> {
    /// Create a new baton.
    ///
    /// You also get a `RegularHandle`, which may be cloned, and one unclonable `EmergencyHandle`.
    /// Regular threads may be added to or removed from the pool freely, but you must not drop the
    /// emergency handle.
    pub fn new(inner: T) -> (Baton<T>, RegularHandle<T>, EmergencyHandle<T>) {
        let (queue_tx, queue_rx) = mpsc::channel();
        let (emergency_tx, emergency_rx) = mpsc::sync_channel(1);
        let baton = Baton {
            queue_rx: queue_rx,
            emergency_tx: emergency_tx,
            inner: inner,
        };
        let regular_handle = RegularHandle(queue_tx);
        let emergency_handle = EmergencyHandle(emergency_rx);
        (baton, regular_handle, emergency_handle)
    }

    /// Pass the baton to another thread.
    ///
    /// If any regular threads are ready to take baton (are currently blocked on `tag_in`), then
    /// one of those will receive it; if not, the emergency thread will receive the baton.
    ///
    /// This function performs a syscall.  On my machine it takes ~1.7 us.
    ///
    /// Note that if all the regular threads are busy, the baton is given to the emergency thread
    /// whether that thread is ready or not.  This means that if *all* threads are busy when
    /// `tag_out` is called, and then a regular thread calls `tag_in`, and then the emergency
    /// thread calls `tag_in`, the baton will be recieved by the emergency thread.
    pub fn tag_out(self) -> Result<(), Error> {
        let (tx, is_emg) = match self.queue_rx.try_recv() {
            Ok(tx) =>
                // Ok, there was a regular thread waiting to take over.
                (tx, false),
            Err(mpsc::TryRecvError::Empty) =>
                // All regular threads are busy.  We'll use the emergency thread this time...
                (self.emergency_tx.clone(), true),
            Err(mpsc::TryRecvError::Disconnected) =>
                // All regular threads are gone!  It's all up to the emergency thread now...
                (self.emergency_tx.clone(), true),
        };
        match tx.try_send(self) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(_)) =>
                if is_emg {
                    Err(Error::EmgThreadIsGone)
                } else {
                    panic!("Impossible: A thread has put a tx on the queue and dropped the rx.
                           There must be a bug in RegularHandle::tag_in.")
                },
            Err(mpsc::TrySendError::Full(_)) =>
                panic!("Impossible: The mvar is already full, but only one baton exists!"),
        }
    }
}

impl<T> RegularHandle<T> {
    /// Declare that this thread is ready to receive the baton.
    ///
    /// Blocks until the baton is passed to it.  This function allocates and performs a syscall.
    //
    // TODO: Consume self, and return the handle when calling tag_out, in order to prevent
    // deadlocks.
    pub fn tag_in(&self) -> Result<Baton<T>, Error> {
        let (tx, rx) = mpsc::sync_channel::<Baton<T>>(1);
        match self.0.send(tx) {
            Ok(()) => {}
            Err(mpsc::SendError(_)) => return Err(Error::BatonIsGone),
        }
        match rx.recv() {
            Ok(x) => Ok(x),
            Err(mpsc::RecvError) =>
                // This case is triggered when (a) the baton is dropped, or (b) the baton-holder
                // pops tx and then drops it.  (b) is impossible, so it must be (a).
                Err(Error::BatonIsGone),
        }
    }
}

impl<T> EmergencyHandle<T> {
    /// Declare that this thread is ready to receive the baton.  Note that this thread will receive
    /// the baton only as a last resort.
    ///
    /// Blocks until the baton is passed to it.  This function allocates and performs a syscall.
    pub fn tag_in(&self) -> Result<Baton<T>, Error> {
        match self.0.recv() {
            Ok(x) => Ok(x),
            Err(mpsc::RecvError) => Err(Error::BatonIsGone),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// Someone has dropped the baton.
    BatonIsGone,
    /// We tried to pass the baton to the emergency thread, but the emergency handle has been
    /// dropped.
    EmgThreadIsGone,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::*;

    #[test]
    fn test_thread_pool() {
        let (mut baton, reg_h, emg_h) = Baton::new((Instant::now(), vec![]));
        let mut threads = vec![];
        for thread_num in 1..4 {
            let h = reg_h.clone();
            threads.push(thread::spawn(move||{
                for i in 0..(10 * thread_num) {
                    let mut baton = h.tag_in().unwrap();
                    baton.inner.1.push(baton.inner.0.elapsed());
                    thread::sleep(Duration::from_millis(3));
                    let ts = Instant::now();
                    baton.inner.0 = ts;
                    baton.tag_out().unwrap();
                    println!("reg {}, iter {:>2}: releasing took {:>5} ns", thread_num, i, ts.elapsed().subsec_nanos());
                    thread::sleep(Duration::from_millis(5));
                }
            }));
        }
        thread::sleep(Duration::from_millis(10));

        // Let's go!
        baton.inner.0 = Instant::now();
        baton.tag_out().unwrap();
        for i in 0..100 {
            let mut baton = emg_h.tag_in().unwrap();
            baton.inner.1.push(baton.inner.0.elapsed());
            thread::sleep(Duration::from_millis(3));
            let ts = Instant::now();
            baton.inner.0 = ts;
            baton.tag_out().unwrap();
            println!("emerg, iter {:>2}: releasing took {:>5} ns", i, ts.elapsed().subsec_nanos());
        }
        for t in threads {
            t.join().unwrap();
        }
        let baton = emg_h.tag_in().unwrap();
        println!("{:?}", baton.inner);
    }
}
