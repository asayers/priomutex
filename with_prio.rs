use std::cmp::Ordering;

/// Tags a term with a priority.
#[derive(Debug)]
pub struct WithPrio<T> {
    pub prio: usize,
    pub inner: T,
}

impl<T> PartialEq for WithPrio<T> {
    fn eq(&self, other: &WithPrio<T>) -> bool { other.prio == self.prio }
}

impl<T> Eq for WithPrio<T> {}

impl<T> PartialOrd for WithPrio<T> {
    fn partial_cmp(&self, other: &WithPrio<T>) -> Option<Ordering> { other.prio.partial_cmp(&self.prio) }
}

impl<T> Ord for WithPrio<T> {
    fn cmp(&self, other: &WithPrio<T>) -> Ordering { other.prio.cmp(&self.prio) }
}
