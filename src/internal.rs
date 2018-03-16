/*!
Internals.  Not meant for consumption.

Internals are exposed for the sake of interest only.  The usual caveats apply:

* No guarantees about API stability
* The user may need to enforce invariants
* The documentation may be inaccurate

*/
use fnv::FnvHasher;
use std::cmp::Ordering;
use std::hash::{Hash,Hasher};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::thread::{self, Thread};

/// Prio guarantees a *total* order, even though the values provided by the user might only be
/// partially ordered.  It does this by also comparing on ThreadId.
///
/// Assumptions: only one Prio per thread; no hash collisions.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Prio {
    prio: usize,
    thread_hash: u64,
}

impl Prio {
    pub fn new(prio: usize) -> Prio {
        let mut s = FnvHasher::default();
        thread::current().id().hash(&mut s);
        Prio {
            prio: prio,
            thread_hash: s.finish(),
        }
    }
}


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
