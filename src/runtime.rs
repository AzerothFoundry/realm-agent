use anyhow::Result;
use crate::{model::{Command,Effects,GameEvent,WorldView},task::TaskEngine,transport::ConnectedClient};

/// Synchronous task loop over the real authenticated protocol transport. HTTP/WebSocket control
/// can enqueue Command values; this owner is the only place that mutates TaskEngine or sends game
/// actions, so group acceptance and task continuation cannot race each other.
pub struct AgentRuntime { pub client: ConnectedClient, pub task: TaskEngine }
impl AgentRuntime {
    pub fn new(client:ConnectedClient)->Self{Self{client,task:TaskEngine::default()}}
    pub fn command(&mut self, command:Command)->Result<Effects>{Ok(self.task.command(command,&self.client.view).map_err(anyhow::Error::msg)?) }
    pub fn step(&mut self)->Result<Effects>{
        let events=self.client.poll_once()?;
        let mut effects=Effects::default();
        for event in events { effects.actions.extend(self.task.event(event,&self.client.view).actions); }
        effects.actions.extend(self.task.tick(&self.client.view).actions);
        for action in effects.actions.clone(){self.client.execute(action)?;}
        Ok(effects)
    }
    pub fn status(&self)->Option<crate::model::TaskStatus>{self.task.status()}
}

#[cfg(test)]
mod tests { use super::*; #[test] fn runtime_type_is_task_owner(){fn assert_send<T:Send>(){} assert_send::<TaskEngine>();} }
