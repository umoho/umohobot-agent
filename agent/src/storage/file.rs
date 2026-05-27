use std::path::PathBuf;

use tokio::fs;
use uuid::Uuid;

use crate::BoxFuture;
use crate::storage::{Storage, StorageResult};
use crate::thread::Thread;

pub struct FileStorage {
    thread_dir: PathBuf,
}

impl FileStorage {
    fn thread_path(&self, id: Uuid) -> PathBuf {
        self.thread_dir.join(format!("thread-{id}.json"))
    }

    fn tmp_path(&self, id: Uuid) -> PathBuf {
        self.thread_dir.join(format!(".thread-{id}.tmp"))
    }
}

impl FileStorage {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let thread_dir = base_dir.into().join("threads");
        std::fs::create_dir_all(&thread_dir).ok();
        Self { thread_dir }
    }
}

impl Storage for FileStorage {
    fn save_thread<'a>(&'a self, thread: &'a Thread) -> BoxFuture<'a, StorageResult<()>> {
        Box::pin(async move {
            let path = self.thread_path(thread.id);
            let tmp = self.tmp_path(thread.id);
            let json = serde_json::to_string(thread)?;
            fs::write(&tmp, &json).await?;
            fs::rename(&tmp, &path).await?;
            Ok(())
        })
    }

    fn load_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, StorageResult<Option<Thread>>> {
        Box::pin(async move {
            let path = self.thread_path(id);
            match fs::read_to_string(&path).await {
                Ok(data) => Ok(Some(serde_json::from_str(&data)?)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }
}
