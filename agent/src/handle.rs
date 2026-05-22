use uuid::Uuid;

use rig_core::OneOrMany;
use rig_core::completion::message::UserContent;
use rig_core::completion::{CompletionModel, Message, Prompt};

use crate::capability::Capability;
use crate::error::AgentError;
use crate::run_turn::{format_message, run_turn_inner};
use crate::runtime::AgentRuntime;
use crate::thread::Thread;
use crate::{BoxFuture, CURRENT_PARENT_THREAD_ID, Usage};

pub trait AgentHandle: Send + Sync {
    fn run_turn<'a>(
        &'a self,
        thread_id: Uuid,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(String, Usage), AgentError>>;
    fn get_or_create_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Thread>;
    fn append_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()>;
    fn set_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()>;
    fn compact_thread<'a>(
        &'a self,
        old_thread_id: Uuid,
        new_thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>>;
    fn capabilities(&self) -> &[Capability];
}

impl<M: CompletionModel + 'static> AgentHandle for AgentRuntime<M> {
    fn run_turn<'a>(
        &'a self,
        thread_id: Uuid,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(String, Usage), AgentError>> {
        Box::pin(async move {
            CURRENT_PARENT_THREAD_ID
                .scope(
                    thread_id,
                    run_turn_inner(&self.agent, &self.threads, thread_id, messages),
                )
                .await
        })
    }

    fn get_or_create_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Thread> {
        Box::pin(async move {
            let threads = self.threads.read().await;
            if let Some(thread) = threads.get(&id) {
                return thread.clone();
            }
            drop(threads);

            let mut threads = self.threads.write().await;
            let thread = Thread::new();
            let clone = thread.clone();
            threads.insert(id, thread);
            clone
        })
    }

    fn append_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut threads = self.threads.write().await;
            if let Some(thread) = threads.get_mut(&thread_id) {
                thread.messages.push(Message::system(text));
            }
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
        })
    }

    fn compact_thread<'a>(
        &'a self,
        old_thread_id: Uuid,
        new_thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>> {
        Box::pin(async move {
            let messages = {
                let threads = self.threads.read().await;
                let thread = threads
                    .get(&old_thread_id)
                    .ok_or(AgentError::ThreadNotFound(old_thread_id))?;
                thread.messages.clone()
            };

            let conversation_text = messages
                .iter()
                .filter_map(|msg| format_message(msg))
                .collect::<Vec<_>>()
                .join("\n");

            let response = self
                .agent
                .prompt(Message::User {
                    content: OneOrMany::one(UserContent::text(conversation_text)),
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

            Ok(response.output)
        })
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
}
