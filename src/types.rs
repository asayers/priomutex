use std::cmp::Ordering;

/// A value `V` with a priority `P`.  The `Ord` impl looks at `P` only.
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
