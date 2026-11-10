use anyhow::{Context,Result,bail};
use clap::Parser;
use serde::Deserialize;
use std::{net::SocketAddr,path::{Path,PathBuf}};

#[derive(Parser)]
pub struct Args {
    #[arg(long)] pub config:PathBuf,
    #[arg(long)] pub host:Option<String>,
    #[arg(long)] pub username:Option<String>,
    #[arg(long)] pub character:Option<String>,
    #[arg(long)] pub api:Option<SocketAddr>,
}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub instance_id:String,
    pub server:Server,
    pub account:Account,
    pub api:Api,
    pub mmaps:PathBuf,
    pub log_dir:PathBuf,
    #[serde(default)] pub task_file:Option<PathBuf>,
    #[serde(default)] pub policy_file:Option<PathBuf>,
    #[serde(default="yes")] pub auto_accept_group:bool,
}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {pub host:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {pub username:String,pub character:String,pub password_env:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Api {pub bind:SocketAddr,pub token_env:String}
fn yes()->bool{true}
impl Config {
    pub fn load(args:Args)->Result<Self>{
        let text=std::fs::read_to_string(&args.config).context("read agent config")?;
        let mut config:Self=serde_json::from_str(&text).context("parse agent config")?;
        if let Some(value)=args.host {config.server.host=value;}
        if let Some(value)=args.username {config.account.username=value;}
        if let Some(value)=args.character {config.account.character=value;}
        if let Some(value)=args.api {config.api.bind=value;}
        if !config.api.bind.ip().is_loopback(){bail!("API bind must be loopback")}
        for value in [&config.instance_id,&config.account.username,&config.account.character,&config.account.password_env,&config.api.token_env] {
            if value.is_empty()||value.len()>128||value.chars().any(char::is_control){bail!("invalid config identifier")}
        }
        // Data and task paths are relative to this config file, not a launcher's working dir.
        let directory=args.config.parent().unwrap_or_else(||Path::new("."));
        for path in [&mut config.mmaps,&mut config.log_dir] {if path.is_relative(){*path=directory.join(&*path);}}
        for path in [&mut config.task_file,&mut config.policy_file].into_iter().flatten(){if path.is_relative(){*path=directory.join(&*path);}}
        Ok(config)
    }
    pub fn password(&self)->Result<String>{std::env::var(&self.account.password_env).context("password environment variable is not set")}
    pub fn api_token(&self)->Result<String>{let token=std::env::var(&self.api.token_env).context("API token environment variable is not set")?;if token.len()<16{bail!("API token must contain at least 16 characters")};Ok(token)}
}
