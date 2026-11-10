use anyhow::Result;
use clap::Parser;
use realm_agent::{api::{ApiState,serve},config::{Args,Config},events::EventHub,model::{Command,Status},runtime::AgentRuntime,transport::ConnectedClient};
use std::{sync::{Arc,Mutex,mpsc},time::{SystemTime,UNIX_EPOCH}};
#[tokio::main]
async fn main()->Result<()> {
    let config=Config::load(Args::parse())?;let token=config.api_token()?;
    std::fs::create_dir_all(&config.log_dir)?;
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let events=Arc::new(EventHub::new(config.log_dir.join(format!("session-{stamp}-{}.jsonl",std::process::id())),1024)?);
    let public=Arc::new(Mutex::new(Status{schema_version:1,instance_id:config.instance_id.clone(),account:config.account.username.clone(),character:config.account.character.clone(),..Default::default()}));
    let password=config.password()?;
    let server=config.server.host.clone();let user=config.account.username.clone();let character=config.account.character.clone();
    let client=tokio::task::spawn_blocking(move||ConnectedClient::login(&server,&user,&password,&character)).await??;
    let(tx,rx)=mpsc::sync_channel(32);
    let api=ApiState{status:public.clone(),commands:tx.clone(),events:events.clone(),token};
    let listener=tokio::net::TcpListener::bind(config.api.bind).await?;
    let address=listener.local_addr()?;
    let runtime=AgentRuntime::new(client,config,events,public)?;
    eprintln!("realm-agent connected; control http://{address}");
    let mut worker=tokio::task::spawn_blocking(move||runtime.run(rx));
    let mut api_worker=tokio::spawn(serve(listener,api));
    tokio::select! {
        result=&mut worker=>{api_worker.abort();result??;},
        result=&mut api_worker=>{request_shutdown(&tx).await;let _=tokio::time::timeout(std::time::Duration::from_secs(5),&mut worker).await;result??;},
        _=tokio::signal::ctrl_c()=>{request_shutdown(&tx).await;let _=tokio::time::timeout(std::time::Duration::from_secs(5),&mut worker).await;api_worker.abort();},
    }
    Ok(())
}
async fn request_shutdown(tx:&mpsc::SyncSender<Command>){
    for _ in 0..100 {
        match tx.try_send(Command::Shutdown) {
            Ok(())|Err(mpsc::TrySendError::Disconnected(_))=>return,
            Err(mpsc::TrySendError::Full(_))=>tokio::time::sleep(std::time::Duration::from_millis(10)).await,
        }
    }
}
