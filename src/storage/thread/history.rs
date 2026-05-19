use super::super::event::EventRecord;
use super::ThreadRecord;

#[derive(Clone, Debug)]
pub struct ThreadHistorySliceRecord {
    pub thread: ThreadRecord,
    pub events: Vec<EventRecord>,
}
