use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use sqlx::{
    FromRow, Sqlite, SqlitePool, Transaction,
    prelude::Connection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tokio::sync::OnceCell;

use crate::platforms::PlatformKind;

use super::super::{
    StorageError,
    event::{EventKind, EventRecord, NewEvent},
    summary::{SummaryRecord, SummaryWrite},
    thread::{InboundMessageRecord, MessageObservation, ThreadRecord, ThreadScope, ThreadState},
    turn::{TurnFinish, TurnRecord, TurnStart, TurnStatus},
    usage::{UsageLedgerRecord, UsageScope, UsageScopeKind, UsageTotalsRecord},
};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[derive(Clone, Debug)]
pub struct SqliteStorage {
    database_path: PathBuf,
    pool: Arc<OnceCell<SqlitePool>>,
}

impl SqliteStorage {
    pub fn new(database_path: PathBuf) -> Self {
        Self {
            database_path,
            pool: Arc::new(OnceCell::new()),
        }
    }

    pub async fn ensure_ready(&self) -> Result<(), StorageError> {
        let _ = self.pool().await?;

        Ok(())
    }

    async fn pool(&self) -> Result<&SqlitePool, StorageError> {
        let database_path = self.database_path.clone();

        self.pool
            .get_or_try_init(move || async move {
                if let Some(parent) = database_path.parent() {
                    fs::create_dir_all(parent).map_err(|source| StorageError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }

                let options = SqliteConnectOptions::new()
                    .filename(&database_path)
                    .create_if_missing(true)
                    .foreign_keys(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .busy_timeout(Duration::from_secs(5));
                let pool = SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect_with(options)
                    .await?;

                MIGRATOR.run(&pool).await?;
                Ok::<SqlitePool, StorageError>(pool)
            })
            .await
    }

    pub async fn observe_message(
        &self,
        inbound: InboundMessageRecord,
    ) -> Result<MessageObservation, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        let now = Utc::now();
        let now_ms = datetime_to_millis(now);
        let lease_until_ms = option_datetime_to_millis(inbound.lease_until)?;
        let thread_key = inbound.scope.thread_key();
        let active = self
            .load_active_thread_rows_tx(&mut tx, thread_key.as_str())
            .await?;
        let reusable = select_reusable_active_thread_row(&active, now_ms);

        let (thread_row, was_new_thread) = if let Some(thread_row) = reusable {
            for row in active.iter().filter(|row| row.id != thread_row.id) {
                self.transition_thread_state_row_tx(&mut tx, row.id, ThreadState::Draining, None)
                    .await?;
            }
            let updated = self
                .touch_thread_row_tx(&mut tx, thread_row.id, now_ms, lease_until_ms)
                .await?;
            (updated, false)
        } else {
            for row in active.iter() {
                self.transition_thread_state_row_tx(&mut tx, row.id, ThreadState::Draining, None)
                    .await?;
            }
            let parent = self
                .load_latest_thread_row_tx(&mut tx, thread_key.as_str())
                .await?;
            let parent_thread_id = parent.as_ref().map(|row| row.id);
            let summary_cursor = parent.as_ref().map(|row| row.summary_cursor).unwrap_or(0);
            let inserted = self
                .insert_thread_row_tx(
                    &mut tx,
                    &inbound.scope,
                    parent_thread_id,
                    summary_cursor,
                    lease_until_ms,
                    now_ms,
                )
                .await?;
            (inserted, true)
        };

        let event_row = self
            .insert_inbound_event_tx(&mut tx, thread_row.id, &inbound, now_ms)
            .await?;

        tx.commit().await?;

        Ok(MessageObservation {
            thread: thread_row.into_record()?,
            event: event_row.into_record()?,
            was_new_thread,
        })
    }

    pub async fn append_event(&self, event: NewEvent) -> Result<EventRecord, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let mut tx = pool.begin().await?;
        let now_ms = option_datetime_to_millis(event.created_at)?;
        let event_row = self.insert_event_tx(&mut tx, event, now_ms).await?;
        tx.commit().await?;

        event_row.into_record()
    }

    pub async fn start_turn(&self, turn: TurnStart) -> Result<TurnRecord, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let mut tx = pool.begin().await?;
        let started_at = turn.started_at.unwrap_or_else(Utc::now);
        let started_at_ms = datetime_to_millis(started_at);
        let lease_until_ms = option_datetime_to_millis(turn.lease_until)?;

        let result = sqlx::query(
            r#"
            INSERT INTO turns (
                thread_id,
                trigger_event_id,
                status,
                started_at,
                ended_at,
                provider,
                model,
                prompt_version,
                context_hash,
                placeholder_message_id,
                final_message_id,
                prompt_tokens,
                completion_tokens,
                tool_calls,
                estimated_usage,
                error_code,
                lease_until
            ) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, NULL, 0, 0, 0, 0, NULL, ?)
            "#,
        )
        .bind(turn.thread_id)
        .bind(turn.trigger_event_id)
        .bind(TurnStatus::Running.as_str())
        .bind(started_at_ms)
        .bind(turn.provider)
        .bind(turn.model)
        .bind(turn.prompt_version)
        .bind(turn.context_hash)
        .bind(turn.placeholder_message_id)
        .bind(lease_until_ms)
        .execute(tx.as_mut())
        .await?;

        let turn_id = result.last_insert_rowid();
        let thread_update = if let Some(lease_until_ms) = lease_until_ms {
            sqlx::query(
                r#"
                UPDATE threads
                SET turn_count = turn_count + 1,
                    lease_until = ?,
                    version = version + 1
                WHERE id = ? AND state = ?
                "#,
            )
            .bind(lease_until_ms)
            .bind(turn.thread_id)
            .bind(ThreadState::Active.as_str())
            .execute(tx.as_mut())
            .await?
        } else {
            sqlx::query(
                r#"
                UPDATE threads
                SET turn_count = turn_count + 1,
                    version = version + 1
                WHERE id = ? AND state = ?
                "#,
            )
            .bind(turn.thread_id)
            .bind(ThreadState::Active.as_str())
            .execute(tx.as_mut())
            .await?
        };

        if thread_update.rows_affected() == 0 {
            return Err(StorageError::InvalidValue {
                kind: "thread_state",
                value: "thread is not active".to_string(),
            });
        }

        let turn_row = self.load_turn_row_tx(&mut tx, turn_id).await?;
        tx.commit().await?;

        turn_row.into_record()
    }

    pub async fn finish_turn(&self, turn: TurnFinish) -> Result<TurnRecord, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let mut tx = pool.begin().await?;
        let original = self.load_turn_row_tx(&mut tx, turn.turn_id).await?;
        let ended_at = turn.ended_at.unwrap_or_else(Utc::now);
        let ended_at_ms = datetime_to_millis(ended_at);
        let prompt_tokens = u64_to_i64(turn.prompt_tokens)?;
        let completion_tokens = u64_to_i64(turn.completion_tokens)?;
        let tool_calls = u64_to_i64(turn.tool_calls)?;
        let updated = sqlx::query(
            r#"
            UPDATE turns
            SET status = ?,
                ended_at = ?,
                final_message_id = ?,
                prompt_tokens = ?,
                completion_tokens = ?,
                tool_calls = ?,
                estimated_usage = ?,
                error_code = ?
            WHERE id = ?
            "#,
        )
        .bind(turn.status.as_str())
        .bind(ended_at_ms)
        .bind(turn.final_message_id)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(tool_calls)
        .bind(turn.estimated_usage)
        .bind(turn.error_code)
        .bind(turn.turn_id)
        .execute(tx.as_mut())
        .await?;

        if updated.rows_affected() == 0 {
            return Err(StorageError::InvalidValue {
                kind: "turn_id",
                value: turn.turn_id.to_string(),
            });
        }

        let lease_clear = sqlx::query(
            r#"
            UPDATE threads
            SET lease_until = NULL,
                version = version + 1
            WHERE id = ?
            "#,
        )
        .bind(original.thread_id)
        .execute(tx.as_mut())
        .await?;

        if lease_clear.rows_affected() == 0 {
            return Err(StorageError::InvalidValue {
                kind: "thread_id",
                value: original.thread_id.to_string(),
            });
        }

        let turn_row = self.load_turn_row_tx(&mut tx, turn.turn_id).await?;
        tx.commit().await?;

        turn_row.into_record()
    }

    pub async fn append_summary(
        &self,
        summary: SummaryWrite,
    ) -> Result<SummaryRecord, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let mut tx = pool.begin().await?;
        let created_at = summary.created_at.unwrap_or_else(Utc::now);
        let created_at_ms = datetime_to_millis(created_at);

        let result = sqlx::query(
            r#"
            INSERT INTO summaries (
                thread_id,
                upto_seq,
                summary_text,
                created_at,
                model,
                prompt_version
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(summary.thread_id)
        .bind(summary.upto_seq)
        .bind(summary.summary_text)
        .bind(created_at_ms)
        .bind(summary.model)
        .bind(summary.prompt_version)
        .execute(tx.as_mut())
        .await?;

        let summary_id = result.last_insert_rowid();
        let thread_update = sqlx::query(
            r#"
            UPDATE threads
            SET summary_cursor = CASE
                    WHEN summary_cursor < ? THEN ?
                    ELSE summary_cursor
                END,
                version = version + 1
            WHERE id = ?
            "#,
        )
        .bind(summary.upto_seq)
        .bind(summary.upto_seq)
        .bind(summary.thread_id)
        .execute(tx.as_mut())
        .await?;

        if thread_update.rows_affected() == 0 {
            return Err(StorageError::InvalidValue {
                kind: "thread_id",
                value: summary.thread_id.to_string(),
            });
        }

        let summary_row = self.load_summary_row_tx(&mut tx, summary_id).await?;
        tx.commit().await?;

        summary_row.into_record()
    }

    pub async fn record_usage(
        &self,
        usage: UsageLedgerRecord,
    ) -> Result<UsageTotalsRecord, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let mut tx = pool.begin().await?;
        let created_at = usage.created_at.unwrap_or_else(Utc::now);
        let created_at_ms = datetime_to_millis(created_at);
        let prompt_tokens = u64_to_i64(usage.prompt_tokens)?;
        let completion_tokens = u64_to_i64(usage.completion_tokens)?;
        let tool_calls = u64_to_i64(usage.tool_calls)?;
        let estimated_prompt_tokens = if usage.estimated { prompt_tokens } else { 0 };
        let estimated_completion_tokens = if usage.estimated {
            completion_tokens
        } else {
            0
        };
        let estimated_tool_calls = if usage.estimated { tool_calls } else { 0 };

        sqlx::query(
            r#"
            INSERT INTO usage_ledger (
                scope_kind,
                scope_id,
                provider,
                turn_id,
                prompt_tokens,
                completion_tokens,
                tool_calls,
                estimated,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(usage.scope.kind.as_str())
        .bind(usage.scope.scope_id.as_str())
        .bind(usage.provider.as_str())
        .bind(usage.turn_id)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(tool_calls)
        .bind(usage.estimated)
        .bind(created_at_ms)
        .execute(tx.as_mut())
        .await?;

        sqlx::query(
            r#"
            INSERT INTO usage_totals (
                scope_kind,
                scope_id,
                provider,
                prompt_tokens,
                completion_tokens,
                tool_calls,
                estimated_prompt_tokens,
                estimated_completion_tokens,
                estimated_tool_calls,
                updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(scope_kind, scope_id, provider) DO UPDATE SET
                prompt_tokens = prompt_tokens + excluded.prompt_tokens,
                completion_tokens = completion_tokens + excluded.completion_tokens,
                tool_calls = tool_calls + excluded.tool_calls,
                estimated_prompt_tokens = estimated_prompt_tokens + excluded.estimated_prompt_tokens,
                estimated_completion_tokens = estimated_completion_tokens + excluded.estimated_completion_tokens,
                estimated_tool_calls = estimated_tool_calls + excluded.estimated_tool_calls,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(usage.scope.kind.as_str())
        .bind(usage.scope.scope_id.as_str())
        .bind(usage.provider.as_str())
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(tool_calls)
        .bind(estimated_prompt_tokens)
        .bind(estimated_completion_tokens)
        .bind(estimated_tool_calls)
        .bind(created_at_ms)
        .execute(tx.as_mut())
        .await?;

        let totals_row = self
            .load_usage_totals_row_tx(&mut tx, &usage.scope, usage.provider.as_str())
            .await?;
        tx.commit().await?;

        totals_row.into_record()
    }

    pub async fn load_active_thread(
        &self,
        scope: &ThreadScope,
    ) -> Result<Option<ThreadRecord>, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let rows = self
            .load_active_thread_rows(pool, scope.thread_key().as_str())
            .await?;
        let row = select_reusable_active_thread_row(&rows, datetime_to_millis(Utc::now()));
        row.map(ThreadRow::into_record).transpose()
    }

    pub async fn load_latest_thread(
        &self,
        scope: &ThreadScope,
    ) -> Result<Option<ThreadRecord>, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let row = self
            .load_latest_thread_row(pool, scope.thread_key().as_str())
            .await?;
        row.map(ThreadRow::into_record).transpose()
    }

    pub async fn close_thread(
        &self,
        thread_id: i64,
        closed_at: Option<DateTime<Utc>>,
    ) -> Result<ThreadRecord, StorageError> {
        self.transition_thread_state(thread_id, ThreadState::Closed, closed_at)
            .await
    }

    pub async fn drain_thread(
        &self,
        thread_id: i64,
        drained_at: Option<DateTime<Utc>>,
    ) -> Result<ThreadRecord, StorageError> {
        self.transition_thread_state(thread_id, ThreadState::Draining, drained_at)
            .await
    }

    async fn transition_thread_state(
        &self,
        thread_id: i64,
        state: ThreadState,
        closed_at: Option<DateTime<Utc>>,
    ) -> Result<ThreadRecord, StorageError> {
        self.ensure_ready().await?;

        let pool = self.pool().await?;
        let mut tx = pool.begin().await?;
        let timestamp_ms = option_datetime_to_millis(closed_at)?;
        self.transition_thread_state_row_tx(&mut tx, thread_id, state, timestamp_ms)
            .await?;

        let thread_row = self.load_thread_row_tx(&mut tx, thread_id).await?;
        tx.commit().await?;

        thread_row.into_record()
    }

    async fn load_active_thread_rows(
        &self,
        executor: &SqlitePool,
        thread_key: &str,
    ) -> Result<Vec<ThreadRow>, StorageError> {
        Ok(sqlx::query_as::<_, ThreadRow>(
            r#"
            SELECT
                id,
                platform,
                chat_id,
                topic_id,
                state,
                opened_at,
                last_activity_at,
                lease_until,
                closed_at,
                parent_thread_id,
                summary_cursor,
                turn_count,
                version
            FROM threads
            WHERE thread_key = ? AND state = ?
            ORDER BY opened_at DESC, id DESC
            "#,
        )
        .bind(thread_key)
        .bind(ThreadState::Active.as_str())
        .fetch_all(executor)
        .await?)
    }

    async fn load_latest_thread_row(
        &self,
        executor: &SqlitePool,
        thread_key: &str,
    ) -> Result<Option<ThreadRow>, StorageError> {
        Ok(sqlx::query_as::<_, ThreadRow>(
            r#"
            SELECT
                id,
                platform,
                chat_id,
                topic_id,
                state,
                opened_at,
                last_activity_at,
                lease_until,
                closed_at,
                parent_thread_id,
                summary_cursor,
                turn_count,
                version
            FROM threads
            WHERE thread_key = ?
            ORDER BY opened_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(thread_key)
        .fetch_optional(executor)
        .await?)
    }

    async fn load_active_thread_rows_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        thread_key: &str,
    ) -> Result<Vec<ThreadRow>, StorageError> {
        Ok(sqlx::query_as::<_, ThreadRow>(
            r#"
            SELECT
                id,
                platform,
                chat_id,
                topic_id,
                state,
                opened_at,
                last_activity_at,
                lease_until,
                closed_at,
                parent_thread_id,
                summary_cursor,
                turn_count,
                version
            FROM threads
            WHERE thread_key = ? AND state = ?
            ORDER BY opened_at DESC, id DESC
            "#,
        )
        .bind(thread_key)
        .bind(ThreadState::Active.as_str())
        .fetch_all(tx.as_mut())
        .await?)
    }

    async fn load_latest_thread_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        thread_key: &str,
    ) -> Result<Option<ThreadRow>, StorageError> {
        Ok(sqlx::query_as::<_, ThreadRow>(
            r#"
            SELECT
                id,
                platform,
                chat_id,
                topic_id,
                state,
                opened_at,
                last_activity_at,
                lease_until,
                closed_at,
                parent_thread_id,
                summary_cursor,
                turn_count,
                version
            FROM threads
            WHERE thread_key = ?
            ORDER BY opened_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(thread_key)
        .fetch_optional(tx.as_mut())
        .await?)
    }

    async fn load_thread_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        thread_id: i64,
    ) -> Result<ThreadRow, StorageError> {
        Ok(sqlx::query_as::<_, ThreadRow>(
            r#"
            SELECT
                id,
                platform,
                chat_id,
                topic_id,
                state,
                opened_at,
                last_activity_at,
                lease_until,
                closed_at,
                parent_thread_id,
                summary_cursor,
                turn_count,
                version
            FROM threads
            WHERE id = ?
            "#,
        )
        .bind(thread_id)
        .fetch_one(tx.as_mut())
        .await?)
    }

    async fn transition_thread_state_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        thread_id: i64,
        state: ThreadState,
        closed_at: Option<i64>,
    ) -> Result<(), StorageError> {
        let updated = match state {
            ThreadState::Closed => {
                sqlx::query(
                    r#"
                UPDATE threads
                SET state = ?,
                    closed_at = ?,
                    lease_until = NULL,
                    version = version + 1
                WHERE id = ?
                "#,
                )
                .bind(state.as_str())
                .bind(closed_at)
                .bind(thread_id)
                .execute(tx.as_mut())
                .await?
            }
            ThreadState::Draining => {
                sqlx::query(
                    r#"
                UPDATE threads
                SET state = ?,
                    lease_until = NULL,
                    version = version + 1
                WHERE id = ?
                "#,
                )
                .bind(state.as_str())
                .bind(thread_id)
                .execute(tx.as_mut())
                .await?
            }
            ThreadState::Active => {
                sqlx::query(
                    r#"
                UPDATE threads
                SET state = ?,
                    version = version + 1
                WHERE id = ?
                "#,
                )
                .bind(state.as_str())
                .bind(thread_id)
                .execute(tx.as_mut())
                .await?
            }
        };

        if updated.rows_affected() == 0 {
            return Err(StorageError::InvalidValue {
                kind: "thread_id",
                value: thread_id.to_string(),
            });
        }

        Ok(())
    }

    async fn insert_thread_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        scope: &ThreadScope,
        parent_thread_id: Option<i64>,
        summary_cursor: i64,
        lease_until_ms: Option<i64>,
        now_ms: i64,
    ) -> Result<ThreadRow, StorageError> {
        let thread_key = scope.thread_key().to_string();
        sqlx::query(
            r#"
            INSERT INTO threads (
                thread_key,
                platform,
                chat_id,
                topic_id,
                state,
                opened_at,
                last_activity_at,
                lease_until,
                closed_at,
                parent_thread_id,
                summary_cursor,
                turn_count,
                version
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, 0, 1)
            "#,
        )
        .bind(thread_key)
        .bind(scope.platform.as_str())
        .bind(scope.chat_id.as_str())
        .bind(scope.topic_id.as_deref())
        .bind(ThreadState::Active.as_str())
        .bind(now_ms)
        .bind(now_ms)
        .bind(lease_until_ms)
        .bind(parent_thread_id)
        .bind(summary_cursor)
        .execute(tx.as_mut())
        .await?;

        self.load_latest_thread_row_tx(tx, scope.thread_key().as_str())
            .await?
            .ok_or_else(|| StorageError::InvalidValue {
                kind: "thread_key",
                value: scope.thread_key().to_string(),
            })
    }

    async fn touch_thread_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        thread_id: i64,
        now_ms: i64,
        lease_until_ms: Option<i64>,
    ) -> Result<ThreadRow, StorageError> {
        let updated = if let Some(lease_until_ms) = lease_until_ms {
            sqlx::query(
                r#"
                UPDATE threads
                SET last_activity_at = ?,
                    lease_until = ?,
                    version = version + 1
                WHERE id = ? AND state = ?
                "#,
            )
            .bind(now_ms)
            .bind(lease_until_ms)
            .bind(thread_id)
            .bind(ThreadState::Active.as_str())
            .execute(tx.as_mut())
            .await?
        } else {
            sqlx::query(
                r#"
                UPDATE threads
                SET last_activity_at = ?,
                    version = version + 1
                WHERE id = ? AND state = ?
                "#,
            )
            .bind(now_ms)
            .bind(thread_id)
            .bind(ThreadState::Active.as_str())
            .execute(tx.as_mut())
            .await?
        };

        if updated.rows_affected() == 0 {
            return Err(StorageError::InvalidValue {
                kind: "thread_id",
                value: thread_id.to_string(),
            });
        }

        self.load_thread_row_tx(tx, thread_id).await
    }

    async fn insert_inbound_event_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        thread_id: i64,
        inbound: &InboundMessageRecord,
        now_ms: i64,
    ) -> Result<EventRow, StorageError> {
        let event = NewEvent {
            thread_id,
            turn_id: None,
            kind: EventKind::InboundMessage,
            sender_id: Some(inbound.sender_id.clone()),
            sender_name: inbound.sender_name.clone(),
            platform_message_id: Some(inbound.platform_message_id.clone()),
            reply_to_platform_message_id: inbound.reply_to_platform_message_id.clone(),
            content: inbound.content.clone(),
            visible_to_model: inbound.visible_to_model,
            created_at: Some(millis_to_datetime(now_ms)?),
        };

        self.insert_event_tx(tx, event, Some(now_ms)).await
    }

    async fn insert_event_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        event: NewEvent,
        created_at_ms: Option<i64>,
    ) -> Result<EventRow, StorageError> {
        let created_at_ms = created_at_ms.unwrap_or_else(|| datetime_to_millis(Utc::now()));
        let seq = self.next_event_seq_tx(tx, event.thread_id).await?;

        let result = sqlx::query(
            r#"
            INSERT INTO events (
                thread_id,
                seq,
                turn_id,
                kind,
                sender_id,
                sender_name,
                platform_message_id,
                reply_to_platform_message_id,
                content_json,
                visible_to_model,
                created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(event.thread_id)
        .bind(seq)
        .bind(event.turn_id)
        .bind(event.kind.as_str())
        .bind(event.sender_id.as_deref())
        .bind(event.sender_name.as_deref())
        .bind(event.platform_message_id.as_deref())
        .bind(event.reply_to_platform_message_id.as_deref())
        .bind(event.content.to_string())
        .bind(event.visible_to_model)
        .bind(created_at_ms)
        .execute(tx.as_mut())
        .await?;

        let event_id = result.last_insert_rowid();
        self.load_event_row_tx(tx, event_id).await
    }

    async fn next_event_seq_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        thread_id: i64,
    ) -> Result<i64, StorageError> {
        let seq: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(seq), 0) + 1
            FROM events
            WHERE thread_id = ?
            "#,
        )
        .bind(thread_id)
        .fetch_one(tx.as_mut())
        .await?;

        Ok(seq)
    }

    async fn load_event_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        event_id: i64,
    ) -> Result<EventRow, StorageError> {
        Ok(sqlx::query_as::<_, EventRow>(
            r#"
            SELECT
                id,
                thread_id,
                seq,
                turn_id,
                kind,
                sender_id,
                sender_name,
                platform_message_id,
                reply_to_platform_message_id,
                content_json,
                visible_to_model,
                created_at
            FROM events
            WHERE id = ?
            "#,
        )
        .bind(event_id)
        .fetch_one(tx.as_mut())
        .await?)
    }

    async fn load_turn_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        turn_id: i64,
    ) -> Result<TurnRow, StorageError> {
        Ok(sqlx::query_as::<_, TurnRow>(
            r#"
            SELECT
                id,
                thread_id,
                trigger_event_id,
                status,
                started_at,
                ended_at,
                provider,
                model,
                prompt_version,
                context_hash,
                placeholder_message_id,
                final_message_id,
                prompt_tokens,
                completion_tokens,
                tool_calls,
                estimated_usage,
                error_code,
                lease_until
            FROM turns
            WHERE id = ?
            "#,
        )
        .bind(turn_id)
        .fetch_one(tx.as_mut())
        .await?)
    }

    async fn load_summary_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        summary_id: i64,
    ) -> Result<SummaryRow, StorageError> {
        Ok(sqlx::query_as::<_, SummaryRow>(
            r#"
            SELECT
                id,
                thread_id,
                upto_seq,
                summary_text,
                created_at,
                model,
                prompt_version
            FROM summaries
            WHERE id = ?
            "#,
        )
        .bind(summary_id)
        .fetch_one(tx.as_mut())
        .await?)
    }

    async fn load_usage_totals_row_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        scope: &UsageScope,
        provider: &str,
    ) -> Result<UsageTotalsRow, StorageError> {
        Ok(sqlx::query_as::<_, UsageTotalsRow>(
            r#"
            SELECT
                scope_kind,
                scope_id,
                provider,
                prompt_tokens,
                completion_tokens,
                tool_calls,
                estimated_prompt_tokens,
                estimated_completion_tokens,
                estimated_tool_calls,
                updated_at
            FROM usage_totals
            WHERE scope_kind = ? AND scope_id = ? AND provider = ?
            "#,
        )
        .bind(scope.kind.as_str())
        .bind(scope.scope_id.as_str())
        .bind(provider)
        .fetch_one(tx.as_mut())
        .await?)
    }
}

#[derive(Clone, Debug, FromRow)]
struct ThreadRow {
    id: i64,
    platform: String,
    chat_id: String,
    topic_id: Option<String>,
    state: String,
    opened_at: i64,
    last_activity_at: i64,
    lease_until: Option<i64>,
    closed_at: Option<i64>,
    parent_thread_id: Option<i64>,
    summary_cursor: i64,
    turn_count: i64,
    version: i64,
}

impl ThreadRow {
    fn is_usable_at(&self, now_ms: i64) -> bool {
        self.lease_until
            .map(|lease_until| lease_until > now_ms)
            .unwrap_or(true)
    }

    fn into_record(self) -> Result<ThreadRecord, StorageError> {
        let platform =
            PlatformKind::from_str(&self.platform).ok_or_else(|| StorageError::InvalidValue {
                kind: "platform",
                value: self.platform.clone(),
            })?;
        let state =
            ThreadState::from_db(&self.state).ok_or_else(|| StorageError::InvalidValue {
                kind: "thread_state",
                value: self.state.clone(),
            })?;
        let scope = ThreadScope::new(platform, self.chat_id.clone(), self.topic_id.clone());

        Ok(ThreadRecord {
            id: self.id,
            scope,
            state,
            opened_at: millis_to_datetime(self.opened_at)?,
            last_activity_at: millis_to_datetime(self.last_activity_at)?,
            lease_until: option_millis_to_datetime(self.lease_until)?,
            closed_at: option_millis_to_datetime(self.closed_at)?,
            parent_thread_id: self.parent_thread_id,
            summary_cursor: self.summary_cursor,
            turn_count: self.turn_count,
            version: self.version,
        })
    }
}

#[derive(Debug, FromRow)]
struct EventRow {
    id: i64,
    thread_id: i64,
    seq: i64,
    turn_id: Option<i64>,
    kind: String,
    sender_id: Option<String>,
    sender_name: Option<String>,
    platform_message_id: Option<String>,
    reply_to_platform_message_id: Option<String>,
    content_json: String,
    visible_to_model: bool,
    created_at: i64,
}

impl EventRow {
    fn into_record(self) -> Result<EventRecord, StorageError> {
        let kind = EventKind::from_db(&self.kind).ok_or_else(|| StorageError::InvalidValue {
            kind: "event_kind",
            value: self.kind.clone(),
        })?;

        Ok(EventRecord {
            id: self.id,
            thread_id: self.thread_id,
            seq: self.seq,
            turn_id: self.turn_id,
            kind,
            sender_id: self.sender_id,
            sender_name: self.sender_name,
            platform_message_id: self.platform_message_id,
            reply_to_platform_message_id: self.reply_to_platform_message_id,
            content: serde_json::from_str(&self.content_json)?,
            visible_to_model: self.visible_to_model,
            created_at: millis_to_datetime(self.created_at)?,
        })
    }
}

#[derive(Debug, FromRow)]
struct TurnRow {
    id: i64,
    thread_id: i64,
    trigger_event_id: i64,
    status: String,
    started_at: i64,
    ended_at: Option<i64>,
    provider: String,
    model: String,
    prompt_version: i64,
    context_hash: Option<String>,
    placeholder_message_id: Option<String>,
    final_message_id: Option<String>,
    prompt_tokens: i64,
    completion_tokens: i64,
    tool_calls: i64,
    estimated_usage: bool,
    error_code: Option<String>,
    lease_until: Option<i64>,
}

impl TurnRow {
    fn into_record(self) -> Result<TurnRecord, StorageError> {
        let status =
            TurnStatus::from_db(&self.status).ok_or_else(|| StorageError::InvalidValue {
                kind: "turn_status",
                value: self.status.clone(),
            })?;

        Ok(TurnRecord {
            id: self.id,
            thread_id: self.thread_id,
            trigger_event_id: self.trigger_event_id,
            status,
            started_at: millis_to_datetime(self.started_at)?,
            ended_at: option_millis_to_datetime(self.ended_at)?,
            provider: self.provider,
            model: self.model,
            prompt_version: self.prompt_version,
            context_hash: self.context_hash,
            placeholder_message_id: self.placeholder_message_id,
            final_message_id: self.final_message_id,
            prompt_tokens: i64_to_u64(self.prompt_tokens)?,
            completion_tokens: i64_to_u64(self.completion_tokens)?,
            tool_calls: i64_to_u64(self.tool_calls)?,
            estimated_usage: self.estimated_usage,
            error_code: self.error_code,
            lease_until: option_millis_to_datetime(self.lease_until)?,
        })
    }
}

#[derive(Debug, FromRow)]
struct SummaryRow {
    id: i64,
    thread_id: i64,
    upto_seq: i64,
    summary_text: String,
    created_at: i64,
    model: String,
    prompt_version: i64,
}

impl SummaryRow {
    fn into_record(self) -> Result<SummaryRecord, StorageError> {
        Ok(SummaryRecord {
            id: self.id,
            thread_id: self.thread_id,
            upto_seq: self.upto_seq,
            summary_text: self.summary_text,
            created_at: millis_to_datetime(self.created_at)?,
            model: self.model,
            prompt_version: self.prompt_version,
        })
    }
}

#[derive(Debug, FromRow)]
struct UsageTotalsRow {
    scope_kind: String,
    scope_id: String,
    provider: String,
    prompt_tokens: i64,
    completion_tokens: i64,
    tool_calls: i64,
    estimated_prompt_tokens: i64,
    estimated_completion_tokens: i64,
    estimated_tool_calls: i64,
    updated_at: i64,
}

impl UsageTotalsRow {
    fn into_record(self) -> Result<UsageTotalsRecord, StorageError> {
        let kind = UsageScopeKind::from_db(&self.scope_kind).ok_or_else(|| {
            StorageError::InvalidValue {
                kind: "usage_scope_kind",
                value: self.scope_kind.clone(),
            }
        })?;

        Ok(UsageTotalsRecord {
            scope: UsageScope::new(kind, self.scope_id),
            provider: self.provider,
            prompt_tokens: i64_to_u64(self.prompt_tokens)?,
            completion_tokens: i64_to_u64(self.completion_tokens)?,
            tool_calls: i64_to_u64(self.tool_calls)?,
            estimated_prompt_tokens: i64_to_u64(self.estimated_prompt_tokens)?,
            estimated_completion_tokens: i64_to_u64(self.estimated_completion_tokens)?,
            estimated_tool_calls: i64_to_u64(self.estimated_tool_calls)?,
            updated_at: millis_to_datetime(self.updated_at)?,
        })
    }
}

fn datetime_to_millis(value: DateTime<Utc>) -> i64 {
    value.timestamp_millis()
}

fn option_datetime_to_millis(value: Option<DateTime<Utc>>) -> Result<Option<i64>, StorageError> {
    match value {
        Some(value) => Ok(Some(datetime_to_millis(value))),
        None => Ok(None),
    }
}

fn millis_to_datetime(value: i64) -> Result<DateTime<Utc>, StorageError> {
    DateTime::<Utc>::from_timestamp_millis(value).ok_or(StorageError::InvalidTimestamp(value))
}

fn option_millis_to_datetime(value: Option<i64>) -> Result<Option<DateTime<Utc>>, StorageError> {
    value.map(millis_to_datetime).transpose()
}

fn u64_to_i64(value: u64) -> Result<i64, StorageError> {
    Ok(i64::try_from(value)?)
}

fn i64_to_u64(value: i64) -> Result<u64, StorageError> {
    Ok(u64::try_from(value)?)
}

fn select_reusable_active_thread_row(rows: &[ThreadRow], now_ms: i64) -> Option<ThreadRow> {
    rows.iter().find(|row| row.is_usable_at(now_ms)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn unique_database_path(prefix: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir()
            .join("umohobot-tests")
            .join(format!("{prefix}-{nonce}"))
            .join("state.sqlite3")
    }

    fn inbound_message(
        scope: ThreadScope,
        platform_message_id: String,
        sender_id: String,
        sender_name: Option<String>,
        text: String,
    ) -> InboundMessageRecord {
        InboundMessageRecord {
            scope,
            platform_message_id,
            sender_id,
            sender_name,
            reply_to_platform_message_id: None,
            content: serde_json::json!({
                "text": text,
            }),
            visible_to_model: true,
            lease_until: None,
        }
    }

    #[tokio::test]
    async fn sqlite_storage_tracks_threads_turns_summaries_and_usage() {
        let storage = SqliteStorage::new(unique_database_path("storage-smoke"));
        let scope = ThreadScope::new(PlatformKind::Telegram, "chat-1", Some("42".to_string()));

        let observation = storage
            .observe_message(inbound_message(
                scope.clone(),
                "msg-1".to_string(),
                "user-1".to_string(),
                Some("Alice".to_string()),
                "hello".to_string(),
            ))
            .await
            .expect("observe message");

        assert!(observation.was_new_thread);
        assert_eq!(observation.event.seq, 1);
        assert_eq!(
            observation.thread.thread_key().as_str(),
            "telegram:chat-1:topic:42"
        );

        let turn = storage
            .start_turn(TurnStart {
                thread_id: observation.thread.id,
                trigger_event_id: observation.event.id,
                provider: "ollama".to_string(),
                model: "llama3.1".to_string(),
                prompt_version: 1,
                context_hash: Some("ctx".to_string()),
                placeholder_message_id: Some("placeholder-1".to_string()),
                lease_until: None,
                started_at: None,
            })
            .await
            .expect("start turn");

        assert_eq!(turn.thread_id, observation.thread.id);
        assert_eq!(turn.trigger_event_id, observation.event.id);
        assert_eq!(turn.status, TurnStatus::Running);

        let finished = storage
            .finish_turn(TurnFinish {
                turn_id: turn.id,
                status: TurnStatus::Completed,
                final_message_id: Some("placeholder-1".to_string()),
                prompt_tokens: 12,
                completion_tokens: 34,
                tool_calls: 2,
                estimated_usage: true,
                error_code: None,
                ended_at: None,
            })
            .await
            .expect("finish turn");

        assert_eq!(finished.status, TurnStatus::Completed);
        assert_eq!(finished.prompt_tokens, 12);
        assert_eq!(finished.completion_tokens, 34);
        assert_eq!(finished.tool_calls, 2);
        assert!(finished.lease_until.is_none());

        let summary = storage
            .append_summary(SummaryWrite {
                thread_id: observation.thread.id,
                upto_seq: observation.event.seq,
                summary_text: "brief summary".to_string(),
                model: "llama3.1".to_string(),
                prompt_version: 1,
                created_at: None,
            })
            .await
            .expect("append summary");

        assert_eq!(summary.thread_id, observation.thread.id);
        assert_eq!(summary.upto_seq, 1);

        let totals = storage
            .record_usage(UsageLedgerRecord {
                scope: UsageScope::thread(observation.thread.thread_key().to_string()),
                provider: "ollama".to_string(),
                turn_id: Some(turn.id),
                prompt_tokens: 12,
                completion_tokens: 34,
                tool_calls: 2,
                estimated: true,
                created_at: None,
            })
            .await
            .expect("record usage");

        assert_eq!(totals.prompt_tokens, 12);
        assert_eq!(totals.completion_tokens, 34);
        assert_eq!(totals.tool_calls, 2);
        assert_eq!(totals.estimated_prompt_tokens, 12);
        assert_eq!(totals.estimated_completion_tokens, 34);
        assert_eq!(totals.estimated_tool_calls, 2);

        let active = storage
            .load_active_thread(&scope)
            .await
            .expect("load active thread")
            .expect("thread should stay active");
        assert_eq!(active.id, observation.thread.id);
        assert_eq!(active.summary_cursor, 1);
        assert_eq!(active.turn_count, 1);
        assert!(active.lease_until.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_observe_message_keeps_one_active_thread() {
        let storage = SqliteStorage::new(unique_database_path("concurrent-observe"));
        let scope = ThreadScope::new(PlatformKind::Telegram, "chat-2", None);
        let barrier = Arc::new(tokio::sync::Barrier::new(4));

        let mut handles = Vec::new();
        for idx in 0..4 {
            let storage = storage.clone();
            let scope = scope.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                storage
                    .observe_message(inbound_message(
                        scope,
                        format!("msg-{idx}"),
                        format!("user-{idx}"),
                        Some(format!("User {idx}")),
                        format!("hello {idx}"),
                    ))
                    .await
                    .expect("observe message")
            }));
        }

        let mut observations = Vec::new();
        for handle in handles {
            observations.push(handle.await.expect("join"));
        }

        let thread_id = observations[0].thread.id;
        assert!(
            observations
                .iter()
                .all(|observation| observation.thread.id == thread_id)
        );
        assert_eq!(
            observations
                .iter()
                .filter(|observation| observation.was_new_thread)
                .count(),
            1
        );

        let pool = storage.pool().await.expect("pool");
        let active_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM threads
            WHERE thread_key = ? AND state = 'active'
            "#,
        )
        .bind(scope.thread_key().as_str())
        .fetch_one(pool)
        .await
        .expect("count active threads");
        assert_eq!(active_count, 1);

        let thread_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM threads
            WHERE thread_key = ?
            "#,
        )
        .bind(scope.thread_key().as_str())
        .fetch_one(pool)
        .await
        .expect("count threads");
        assert_eq!(thread_count, 1);
    }

    #[tokio::test]
    async fn expired_lease_starts_new_thread() {
        let storage = SqliteStorage::new(unique_database_path("lease-expiry"));
        let scope = ThreadScope::new(PlatformKind::Telegram, "chat-3", None);

        let first = storage
            .observe_message(inbound_message(
                scope.clone(),
                "msg-1".to_string(),
                "user-1".to_string(),
                Some("Alice".to_string()),
                "hello".to_string(),
            ))
            .await
            .expect("observe first message");

        let pool = storage.pool().await.expect("pool");
        let expired_lease_until = datetime_to_millis(Utc::now() - chrono::Duration::seconds(5));
        sqlx::query(
            r#"
            UPDATE threads
            SET lease_until = ?
            WHERE id = ?
            "#,
        )
        .bind(expired_lease_until)
        .bind(first.thread.id)
        .execute(pool)
        .await
        .expect("expire lease");

        let active_before = storage
            .load_active_thread(&scope)
            .await
            .expect("load active thread before expiry");
        assert!(active_before.is_none());

        let second = storage
            .observe_message(inbound_message(
                scope.clone(),
                "msg-2".to_string(),
                "user-2".to_string(),
                Some("Bob".to_string()),
                "after timeout".to_string(),
            ))
            .await
            .expect("observe second message");

        assert!(second.was_new_thread);
        assert_ne!(second.thread.id, first.thread.id);

        let active_after = storage
            .load_active_thread(&scope)
            .await
            .expect("load active thread after expiry")
            .expect("new active thread");
        assert_eq!(active_after.id, second.thread.id);

        let old_state: String = sqlx::query_scalar(
            r#"
            SELECT state
            FROM threads
            WHERE id = ?
            "#,
        )
        .bind(first.thread.id)
        .fetch_one(pool)
        .await
        .expect("load old thread state");
        assert_eq!(old_state, "draining");
    }
}
