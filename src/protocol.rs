//! Protocol adapter boundary. The task engine never owns wire objects; this module translates
//! normal client requests and authoritative packets into the value-only WorldView/GameEvent API.
use crate::model::{Action,GameEvent,WorldView};
use anyhow::{Result,bail};

pub trait ClientTransport {
    fn send(&mut self, action:Action)->Result<()>;
    fn poll(&mut self, view:&mut WorldView)->Result<Vec<GameEvent>>;
}
pub struct AgentLoop<T:ClientTransport>{pub transport:T,pub view:WorldView}
impl<T:ClientTransport> AgentLoop<T>{
 pub fn apply(&mut self,actions:Vec<Action>)->Result<()> {for action in actions{self.transport.send(action)?;}Ok(())}
 pub fn poll(&mut self)->Result<Vec<GameEvent>> {self.transport.poll(&mut self.view)}
 pub fn validate_selected_character(&self)->Result<()> {if self.view.self_guid==0||self.view.character.is_empty(){bail!("character identity not loaded")};if !matches!(self.view.class,1|8){bail!("only mage and warrior are supported")};Ok(())}
}
