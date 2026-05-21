use moka::sync::Cache;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct DataBuffer {
    inner: Cache<String, Vec<u8>>,
}

impl DataBuffer {
    pub fn new() -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(200)
                .time_to_live(Duration::from_secs(600))
                .build(),
        }
    }

    pub fn store(&self, data: Vec<u8>) -> String {
        let key = Uuid::new_v4().to_string();
        self.inner.insert(key.clone(), data);
        key
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.get(key)
    }
}

impl Default for DataBuffer {
    fn default() -> Self {
        Self::new()
    }
}
