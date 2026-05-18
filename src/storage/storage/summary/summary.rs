use chrono::{DateTime, Utc};

#[derive(Clone, Debug)]
pub struct SummaryWrite {
    pub thread_id: i64,
    pub upto_seq: i64,
    pub summary_text: String,
    pub model: String,
    pub prompt_version: i64,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct SummaryRecord {
    pub id: i64,
    pub thread_id: i64,
    pub upto_seq: i64,
    pub summary_text: String,
    pub created_at: DateTime<Utc>,
    pub model: String,
    pub prompt_version: i64,
}
