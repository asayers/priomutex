use std::hash::{Hash, Hasher};
use std::thread::ThreadId;

/// An evil little function to extract the integer from inside an opaque `ThreadId`.
///
/// This works in rust 1.23, but I can't guarantee that it won't break in the future.
pub fn extract_thread_id(x: ThreadId) -> u64 {
    let mut extractor = ThreadIdExtractor(0);
    x.hash(&mut extractor);
    extractor.0
}

struct ThreadIdExtractor(u64);
impl Hasher for ThreadIdExtractor {
    fn write_u64(&mut self, i: u64)     { self.0 = i; }
    fn finish(&self) -> u64             { panic!("ThreadIdExtractor: unexpected input"); }
    fn write(&mut self, _: &[u8])       { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_u8(&mut self, _: u8)       { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_u16(&mut self, _: u16)     { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_u32(&mut self, _: u32)     { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_usize(&mut self, _: usize) { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_i8(&mut self, _: i8)       { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_i16(&mut self, _: i16)     { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_i32(&mut self, _: i32)     { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_i64(&mut self, _: i64)     { panic!("ThreadIdExtractor: unexpected input"); }
    fn write_isize(&mut self, _: isize) { panic!("ThreadIdExtractor: unexpected input"); }
}
