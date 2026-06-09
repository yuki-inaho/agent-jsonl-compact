use serde_json::{Map, Number, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Counter {
    counts: HashMap<String, usize>,
}

impl Counter {
    pub fn inc<S: Into<String>>(&mut self, key: S) {
        *self.counts.entry(key.into()).or_insert(0) += 1;
    }

    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }

    pub fn most_common(&self) -> Vec<(String, usize)> {
        let mut items: Vec<_> = self.counts.iter().map(|(k, v)| (k.clone(), *v)).collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items
    }

    pub fn to_json_object_by_count(&self) -> Value {
        let mut object = Map::new();
        for (key, count) in self.most_common() {
            object.insert(key, Value::Number(Number::from(count as u64)));
        }
        Value::Object(object)
    }
}
