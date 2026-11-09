use anyhow::Result;
use clap::Parser;
use realm_agent::{events::EventHub,model::{Command,Status},api::{ApiState,serve}};
use std::{net::SocketAddr,path::PathBuf,sync::{Arc,Mutex,mpsc::sync_channel}};
#[derive(Parser,Debug)]
struct Args { #[arg(long,default_value="127.0.0.1:18112")] api:SocketAddr, #[arg(long,env="REALM_AGENT_API_TOKEN")] token:String, #[arg(long,default_value="run/events.jsonl")] events:PathBuf }
#[tokio::main]
async fn main()->Result<()>{let args=Args::parse();if args.token.len()<16{anyhow::bail!("REALM_AGENT_API_TOKEN must be at least 16 characters")};if !args.api.ip().is_loopback(){anyhow::bail!("API is loopback-only")};let hub=Arc::new(EventHub::new(args.events,1024)?);let(status_tx,status_rx)=sync_channel(32);let status=Arc::new(Mutex::new(Status{schema_version:1,..Default::default()}));let api=ApiState{status:status.clone(),commands:status_tx,events:hub.clone(),token:args.token};let listener=tokio::net::TcpListener::bind(args.api).await?;let api_task=tokio::spawn(serve(listener,api));
 // Runtime wiring is deliberately explicit: credentials and a concrete WorldSession transport
 // are supplied by the embedding process. This binary currently exposes the safe API surface.
 while let Ok(command)=status_rx.recv(){hub.emit(None,"agent.command_received",serde_json::json!({"command":format!("{:?}",command)}))?;if matches!(command,Command::Shutdown){break;}}
 api_task.abort();Ok(())}
