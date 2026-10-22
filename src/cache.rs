use std::collections::VecDeque;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub source: String,
    pub target: String,
    pub text: String,
    pub profile: String,
}
pub struct Cache {
    entries: VecDeque<(Key, String)>,
    capacity: usize,
}
impl Cache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity,
        }
    }
    pub fn get(&mut self, key: &Key) -> Option<String> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(index)?;
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }
    pub fn put(&mut self, key: Key, value: String) {
        if self.capacity == 0 || value.trim().is_empty() {
            return;
        }
        self.entries.retain(|(k, _)| k != &key);
        self.entries.push_back((key, value));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.entries.iter().any(|(stored, _)| stored == key)
    }
}
