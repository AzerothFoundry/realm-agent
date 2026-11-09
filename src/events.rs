use crate::model::AgentEvent;
use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::{collections::VecDeque, fs::{File,OpenOptions}, io::Write, path::PathBuf, sync::Mutex, time::{SystemTime,UNIX_EPOCH}};
use tokio::sync::broadcast;
const MAX_BYTES:u64=5*1024*1024;
struct Inner { file:File, bytes:u64, sequence:u64, ring:VecDeque<AgentEvent> }
pub struct EventHub { path:PathBuf, capacity:usize, inner:Mutex<Inner>, sender:broadcast::Sender<AgentEvent> }
#[derive(Serialize)]
pub struct History {pub events:Vec<AgentEvent>,pub gap:bool,pub oldest:u64,pub latest:u64}
impl EventHub {
 pub fn new(path:PathBuf,capacity:usize)->Result<Self>{
  if capacity==0||capacity>4096 {bail!("event capacity out of range")}
  // A fresh per-session path never overwrites an existing private file or symlink.
  let file=secure_create(&path)?; let(sender,_)=broadcast::channel(capacity);
  Ok(Self{path,capacity,inner:Mutex::new(Inner{file,bytes:0,sequence:0,ring:VecDeque::new()}),sender})
 }
 pub fn emit(&self,task_id:Option<u64>,kind:&str,detail:Value)->Result<AgentEvent>{
  if kind.is_empty()||kind.len()>96||kind.chars().any(char::is_control){bail!("invalid event kind")}
  reject_secrets(&detail)?;
  let mut inner=self.inner.lock().map_err(|_|anyhow::anyhow!("event log poisoned"))?;
  let event=AgentEvent{schema_version:1,id:inner.sequence.checked_add(1).ok_or_else(||anyhow::anyhow!("event sequence overflow"))?,time_ms:SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().try_into()?,task_id,kind:kind.into(),detail};
  let mut bytes=serde_json::to_vec(&event)?;if bytes.len()>65536{bail!("event exceeds 64 KiB")};bytes.push(b'\n');
  if inner.bytes+bytes.len() as u64>MAX_BYTES {
   inner.file.flush()?;
   // No existing rotation target may be a symlink or non-regular file.
   for n in 1..=5{let p=self.path.with_extension(format!("jsonl.{n}"));if let Ok(m)=std::fs::symlink_metadata(&p){if !m.file_type().is_file(){bail!("unsafe rotation target")}}}
   let oldest=self.path.with_extension("jsonl.5");if oldest.exists(){std::fs::remove_file(oldest)?;}
   for n in (1..5).rev(){let from=self.path.with_extension(format!("jsonl.{n}"));if from.exists(){std::fs::rename(from,self.path.with_extension(format!("jsonl.{}",n+1)))?;}}
   std::fs::rename(&self.path,self.path.with_extension("jsonl.1"))?;
   inner.file=secure_create(&self.path)?;inner.bytes=0;
  }
  inner.file.write_all(&bytes)?;inner.file.flush()?;inner.bytes+=bytes.len()as u64;inner.sequence=event.id;
  if inner.ring.len()==self.capacity{inner.ring.pop_front();}inner.ring.push_back(event.clone());
  let _=self.sender.send(event.clone());Ok(event)
 }
 pub fn history(&self,after:u64,limit:usize)->Result<History>{let i=self.inner.lock().map_err(|_|anyhow::anyhow!("event log poisoned"))?;let oldest=i.ring.front().map_or(0,|e|e.id);Ok(History{events:i.ring.iter().filter(|e|e.id>after).take(limit.min(256)).cloned().collect(),gap:after!=0&&(after.saturating_add(1)<oldest||after>i.sequence),oldest,latest:i.sequence})}
 pub fn subscribe(&self)->broadcast::Receiver<AgentEvent>{self.sender.subscribe()}
}
fn secure_create(path:&PathBuf)->Result<File>{let mut options=OpenOptions::new();options.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}Ok(options.open(path)?)}
fn reject_secrets(v:&Value)->Result<()>{match v{Value::Object(m)=>for(k,v)in m{if ["password","token","secret","session_key","raw_packet","raw_chat"].iter().any(|s|k.to_ascii_lowercase().contains(s)){bail!("sensitive event field forbidden")}reject_secrets(v)?;},Value::Array(a)=>for v in a{reject_secrets(v)?},_=>{}}Ok(())}
