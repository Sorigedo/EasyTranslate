#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub target: u64,
    pub text: String,
    pub language: String,
}
#[derive(Default)]
pub struct Draft {
    pub generation: u64,
    pub current: Option<Snapshot>,
    pub translation: Option<String>,
}
impl Draft {
    pub fn observe(&mut self, snapshot: Option<Snapshot>) -> bool {
        let snapshot = snapshot.filter(|s| !s.text.trim().is_empty());
        if self.current == snapshot {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.current = snapshot;
        self.translation = None;
        true
    }
    pub fn accept(&mut self, generation: u64, text: String) -> bool {
        if generation != self.generation || self.current.is_none() || text.trim().is_empty() {
            return false;
        }
        self.translation = Some(text);
        true
    }
    pub fn replacement(&self, live: &Snapshot) -> Option<&str> {
        (self.current.as_ref() == Some(live))
            .then_some(self.translation.as_deref())
            .flatten()
    }
}
