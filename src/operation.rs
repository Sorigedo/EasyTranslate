use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[derive(Default)]
pub struct Operation {
    generation: u64,
    token: Arc<AtomicU64>,
}
impl Operation {
    pub fn cancellation(&self) -> (Arc<AtomicU64>, u64) {
        (self.token.clone(), self.generation)
    }
    pub fn begin(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.token.store(self.generation, Ordering::Relaxed);
        self.generation
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
}
