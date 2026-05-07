use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct BTreeIndex<K: Ord, V> {
    inner: BTreeMap<K, Vec<V>>,
}

impl<K: Ord + Clone, V: Clone + Ord> BTreeIndex<K, V> {
    pub fn insert(&mut self, key: K, value: V) {
        self.inner.entry(key).or_default().push(value);
    }
    pub fn remove(&mut self, key: &K, value: &V) {
        if let Some(v) = self.inner.get_mut(key) {
            v.retain(|x| x != value);
            if v.is_empty() {
                self.inner.remove(key);
            }
        }
    }
    pub fn eq(&self, key: &K) -> Vec<V> {
        self.inner.get(key).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_ops() {
        let mut t = BTreeIndex::default();
        t.insert(1, 10);
        assert_eq!(t.eq(&1), vec![10]);
    }
}
