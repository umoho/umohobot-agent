use uuid::Uuid;

use rig_core::OneOrMany;
use rig_core::completion::message::UserContent;
use rig_core::completion::{Message, Prompt};

use crate::capability::Capability;
use crate::error::AgentError;
use crate::run_turn::run_turn_inner;
use crate::runtime::AgentRuntime;
use crate::thread::Thread;
use crate::{BoxFuture, CURRENT_PARENT_THREAD_ID, Usage};

pub trait AgentHandle: Send + Sync {
    fn run_turn<'a>(
        &'a self,
        thread_id: Uuid,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(String, Usage), AgentError>>;
    fn create_thread<'a>(&'a self) -> BoxFuture<'a, Thread>;
    fn get_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Option<Thread>>;
    fn append_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()>;
    fn set_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()>;
    fn get_thread_messages<'a>(
        &'a self,
        thread_id: Uuid,
    ) -> BoxFuture<'a, Result<Vec<Message>, AgentError>>;
    fn compact_thread<'a>(
        &'a self,
        formatted_text: &'a str,
        old_thread_id: Uuid,
        new_thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>>;
    fn capabilities(&self) -> &[Capability];
}

impl AgentHandle for AgentRuntime {
    fn run_turn<'a>(
        &'a self,
        thread_id: Uuid,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(String, Usage), AgentError>> {
        Box::pin(async move {
            let result = CURRENT_PARENT_THREAD_ID
                .scope(
                    thread_id,
                    run_turn_inner(&self.agent, &self.threads, thread_id, messages),
                )
                .await;
            self.persist_thread(thread_id).await;
            result
        })
    }

    fn create_thread<'a>(&'a self) -> BoxFuture<'a, Thread> {
        Box::pin(async move {
            let thread = Thread::new();
            let id = thread.id;
            let clone = thread.clone();
            self.threads.write().await.insert(id, thread);
            self.persist_thread(id).await;
            clone
        })
    }

    fn get_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Option<Thread>> {
        Box::pin(async move { self.threads.read().await.get(&id).cloned() })
    }

    fn append_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut threads = self.threads.write().await;
            if let Some(thread) = threads.get_mut(&thread_id) {
                thread.messages.push(Message::system(text));
            }
            drop(threads);
            self.persist_thread(thread_id).await;
        })
    }

    fn set_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut threads = self.threads.write().await;
            if let Some(thread) = threads.get_mut(&thread_id) {
                let system_idx = thread
                    .messages
                    .iter()
                    .position(|m| matches!(m, Message::System { .. }));
                if let Some(idx) = system_idx {
                    thread.messages[idx] = Message::system(text);
                } else {
                    thread.messages.push(Message::system(text));
                }
            }
            drop(threads);
            self.persist_thread(thread_id).await;
        })
    }

    fn get_thread_messages<'a>(
        &'a self,
        thread_id: Uuid,
    ) -> BoxFuture<'a, Result<Vec<Message>, AgentError>> {
        Box::pin(async move {
            let threads = self.threads.read().await;
            let thread = threads
                .get(&thread_id)
                .ok_or(AgentError::ThreadNotFound(thread_id))?;
            Ok(thread.messages.clone())
        })
    }

    fn compact_thread<'a>(
        &'a self,
        formatted_text: &'a str,
        old_thread_id: Uuid,
        new_thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>> {
        Box::pin(async move {
            let response = self
                .agent
                .prompt(Message::User {
                    content: OneOrMany::one(UserContent::text(formatted_text.to_string())),
                })
                .with_history(vec![Message::system(compact_prompt)])
                .extended_details()
                .await?;

            {
                let mut threads = self.threads.write().await;
                if let Some(old) = threads.get_mut(&old_thread_id) {
                    old.close();
                }
                if let Some(new) = threads.get_mut(&new_thread_id) {
                    new.prev_thread_id = Some(old_thread_id);
                }
            }

            self.persist_thread(old_thread_id).await;
            self.persist_thread(new_thread_id).await;

            Ok(response.output)
        })
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
}
