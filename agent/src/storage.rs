pub mod file;

use uuid::Uuid;

use crate::BoxFuture;
use crate::thread::Thread;

pub type StorageResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub trait Storage: Send + Sync {
    fn save_thread<'a>(&'a self, thread: &'a Thread) -> BoxFuture<'a, StorageResult<()>>;
    fn load_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, StorageResult<Option<Thread>>>;
}
