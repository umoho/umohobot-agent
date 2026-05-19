use std::{env, path::PathBuf};

use thiserror::Error;

pub mod event;
pub mod sqlite;
pub mod summary;
pub mod thread;
pub mod turn;
pub mod usage;

pub use event::{EventKind, EventRecord, NewEvent};
pub use sqlite::SqliteStorage;
pub use summary::{SummaryRecord, SummaryWrite};
pub use thread::{
    InboundMessageRecord, MessageObservation, ThreadHistorySliceRecord, ThreadKey, ThreadRecord,
    ThreadScope, ThreadState,
};
pub use turn::{TurnFinish, TurnRecord, TurnStart, TurnStatus};
pub use usage::{UsageLedgerRecord, UsageScope, UsageScopeKind, UsageTotalsRecord};

#[derive(Clone, Debug)]
pub struct Storage {
    data_dir: PathBuf,
    backend: StorageBackend,
}

#[derive(Clone, Debug)]
enum StorageBackend {
    Sqlite(SqliteStorage),
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error when preparing {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid {kind} value: {value}")]
    InvalidValue { kind: &'static str, value: String },
    #[error("timestamp is out of range: {0}")]
    InvalidTimestamp(i64),
    #[error("integer conversion failed: {0}")]
    IntConversion(#[from] std::num::TryFromIntError),
}

impl Storage {
    pub fn new(data_dir: Option<PathBuf>) -> Self {
        let data_dir = data_dir
            .or_else(|| {
                env::var("DATA_DIR")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("data"));
        let database_path = data_dir.join("umohobot.sqlite3");

        Self {
            data_dir,
            backend: StorageBackend::sqlite(database_path),
        }
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn data_dir(&self) -> Option<&PathBuf> {
        Some(&self.data_dir)
    }

    pub async fn ensure_ready(&self) -> Result<(), StorageError> {
        self.backend.ensure_ready().await
    }

    pub async fn observe_message(
        &self,
        message: InboundMessageRecord,
    ) -> Result<MessageObservation, StorageError> {
        self.backend.observe_message(message).await
    }

    pub async fn append_event(&self, event: NewEvent) -> Result<EventRecord, StorageError> {
        self.backend.append_event(event).await
    }

    pub async fn start_turn(&self, turn: TurnStart) -> Result<TurnRecord, StorageError> {
        self.backend.start_turn(turn).await
    }

    pub async fn finish_turn(&self, turn: TurnFinish) -> Result<TurnRecord, StorageError> {
        self.backend.finish_turn(turn).await
    }

    pub async fn set_turn_placeholder_message(
        &self,
        turn_id: i64,
        placeholder_message_id: Option<String>,
    ) -> Result<TurnRecord, StorageError> {
        self.backend
            .set_turn_placeholder_message(turn_id, placeholder_message_id)
            .await
    }

    pub async fn append_summary(
        &self,
        summary: SummaryWrite,
    ) -> Result<SummaryRecord, StorageError> {
        self.backend.append_summary(summary).await
    }

    pub async fn load_latest_summary(
        &self,
        thread_id: i64,
    ) -> Result<Option<SummaryRecord>, StorageError> {
        self.backend.load_latest_summary(thread_id).await
    }

    pub async fn load_latest_summary_before_seq(
        &self,
        thread_id: i64,
        upto_seq: i64,
    ) -> Result<Option<SummaryRecord>, StorageError> {
        self.backend
            .load_latest_summary_before_seq(thread_id, upto_seq)
            .await
    }

    pub async fn load_recent_visible_events(
        &self,
        thread_id: i64,
        after_seq_exclusive: i64,
        before_seq_exclusive: i64,
    ) -> Result<Vec<EventRecord>, StorageError> {
        self.backend
            .load_recent_visible_events(thread_id, after_seq_exclusive, before_seq_exclusive)
            .await
    }

    pub async fn load_thread_history_slice(
        &self,
        scope: &ThreadScope,
        after_seq_exclusive: i64,
        before_seq_exclusive: i64,
        limit: i64,
    ) -> Result<Option<ThreadHistorySliceRecord>, StorageError> {
        self.backend
            .load_thread_history_slice(scope, after_seq_exclusive, before_seq_exclusive, limit)
            .await
    }

    pub async fn load_thread(&self, thread_id: i64) -> Result<Option<ThreadRecord>, StorageError> {
        self.backend.load_thread(thread_id).await
    }

    pub async fn record_usage(
        &self,
        usage: UsageLedgerRecord,
    ) -> Result<UsageTotalsRecord, StorageError> {
        self.backend.record_usage(usage).await
    }

    pub async fn load_active_thread(
        &self,
        scope: &ThreadScope,
    ) -> Result<Option<ThreadRecord>, StorageError> {
        self.backend.load_active_thread(scope).await
    }

    pub async fn load_latest_thread(
        &self,
        scope: &ThreadScope,
    ) -> Result<Option<ThreadRecord>, StorageError> {
        self.backend.load_latest_thread(scope).await
    }

    pub async fn close_thread(
        &self,
        thread_id: i64,
        closed_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<ThreadRecord, StorageError> {
        self.backend.close_thread(thread_id, closed_at).await
    }

    pub async fn drain_thread(
        &self,
        thread_id: i64,
        drained_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<ThreadRecord, StorageError> {
        self.backend.drain_thread(thread_id, drained_at).await
    }
}

impl StorageBackend {
    fn sqlite(database_path: PathBuf) -> Self {
        Self::Sqlite(SqliteStorage::new(database_path))
    }

    async fn ensure_ready(&self) -> Result<(), StorageError> {
        match self {
            Self::Sqlite(storage) => storage.ensure_ready().await,
        }
    }

    async fn observe_message(
        &self,
        message: InboundMessageRecord,
    ) -> Result<MessageObservation, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.observe_message(message).await,
        }
    }

    async fn append_event(&self, event: NewEvent) -> Result<EventRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.append_event(event).await,
        }
    }

    async fn start_turn(&self, turn: TurnStart) -> Result<TurnRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.start_turn(turn).await,
        }
    }

    async fn finish_turn(&self, turn: TurnFinish) -> Result<TurnRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.finish_turn(turn).await,
        }
    }

    async fn set_turn_placeholder_message(
        &self,
        turn_id: i64,
        placeholder_message_id: Option<String>,
    ) -> Result<TurnRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => {
                storage
                    .set_turn_placeholder_message(turn_id, placeholder_message_id)
                    .await
            }
        }
    }

    async fn append_summary(&self, summary: SummaryWrite) -> Result<SummaryRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.append_summary(summary).await,
        }
    }

    async fn load_latest_summary(
        &self,
        thread_id: i64,
    ) -> Result<Option<SummaryRecord>, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.load_latest_summary(thread_id).await,
        }
    }

    async fn load_latest_summary_before_seq(
        &self,
        thread_id: i64,
        upto_seq: i64,
    ) -> Result<Option<SummaryRecord>, StorageError> {
        match self {
            Self::Sqlite(storage) => {
                storage
                    .load_latest_summary_before_seq(thread_id, upto_seq)
                    .await
            }
        }
    }

    async fn load_recent_visible_events(
        &self,
        thread_id: i64,
        after_seq_exclusive: i64,
        before_seq_exclusive: i64,
    ) -> Result<Vec<EventRecord>, StorageError> {
        match self {
            Self::Sqlite(storage) => {
                storage
                    .load_recent_visible_events(
                        thread_id,
                        after_seq_exclusive,
                        before_seq_exclusive,
                    )
                    .await
            }
        }
    }

    async fn load_thread_history_slice(
        &self,
        scope: &ThreadScope,
        after_seq_exclusive: i64,
        before_seq_exclusive: i64,
        limit: i64,
    ) -> Result<Option<ThreadHistorySliceRecord>, StorageError> {
        match self {
            Self::Sqlite(storage) => {
                storage
                    .load_thread_history_slice(
                        scope,
                        after_seq_exclusive,
                        before_seq_exclusive,
                        limit,
                    )
                    .await
            }
        }
    }

    async fn load_thread(&self, thread_id: i64) -> Result<Option<ThreadRecord>, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.load_thread(thread_id).await,
        }
    }

    async fn record_usage(
        &self,
        usage: UsageLedgerRecord,
    ) -> Result<UsageTotalsRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.record_usage(usage).await,
        }
    }

    async fn load_active_thread(
        &self,
        scope: &ThreadScope,
    ) -> Result<Option<ThreadRecord>, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.load_active_thread(scope).await,
        }
    }

    async fn load_latest_thread(
        &self,
        scope: &ThreadScope,
    ) -> Result<Option<ThreadRecord>, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.load_latest_thread(scope).await,
        }
    }

    async fn close_thread(
        &self,
        thread_id: i64,
        closed_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<ThreadRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.close_thread(thread_id, closed_at).await,
        }
    }

    async fn drain_thread(
        &self,
        thread_id: i64,
        drained_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<ThreadRecord, StorageError> {
        match self {
            Self::Sqlite(storage) => storage.drain_thread(thread_id, drained_at).await,
        }
    }
}
