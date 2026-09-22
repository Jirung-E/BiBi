use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub(crate) struct Sessions<T> {
    entries: Mutex<HashMap<String, (Instant, T)>>,
}
impl<T> Default for Sessions<T> {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}
impl<T> Sessions<T> {
    pub async fn take(&self, key: &str) -> Option<T> {
        let mut entries = self.entries.lock().await;
        entries.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(1800));
        entries.remove(key).map(|(_, v)| v)
    }
    pub async fn put(&self, key: &str, value: T) {
        let mut entries = self.entries.lock().await;
        if entries.len() >= 16
            && let Some(old) = entries
                .iter()
                .min_by_key(|(_, (time, _))| *time)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&old);
        }
        entries.insert(key.into(), (Instant::now(), value));
    }
    pub async fn clear(&self) {
        self.entries.lock().await.clear();
    }
}
