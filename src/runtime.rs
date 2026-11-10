use anyhow::{Context, Result};
use benilla_protocol::{messages::opcode, Poll};
use crate::{config::Config,events::EventHub,model::{Action,Command,Effects,GameEvent,Kind,Position,Status,TaskSpec},navigation::{Follower,Navigator},task::TaskEngine,transport::ConnectedClient};
use serde_json::json;
use std::{sync::{Arc,Mutex,mpsc::{self,Receiver,TryRecvError}},time::{Duration,Instant}};

struct Leg { follower:Follower, goal:Position, task:u64, phase:String }
/// One actor owns world state, tasks and all packet writes. A bounded reader thread can wait for
/// network packets while this loop continues to service control, timers and normal movement.
pub struct AgentRuntime {
    strategy:crate::policy_runtime::PolicyRuntime,
    pub client:ConnectedClient, pub task:TaskEngine,
    packets:Receiver<Result<Poll,String>>,navigator:Navigator,leg:Option<Leg>,
    clock:Instant,last_move:Instant,last_heartbeat:Instant,last_status:Instant,last_ping:Instant,ping_sequence:u32,
    events:Arc<EventHub>,public:Arc<Mutex<Status>>,config:Config,
    last_group:Vec<String>,autostart:Option<TaskSpec>,shutdown:bool,memory_at:Instant,resident_kib:Option<u64>,native_moving:bool,position_revision:u64,
}
impl AgentRuntime {
    pub fn new(mut client:ConnectedClient,config:Config,events:Arc<EventHub>,public:Arc<Mutex<Status>>)->Result<Self>{
        let mut reader=client.take_reader()?;let(tx,rx)=mpsc::sync_channel(128);
        std::thread::Builder::new().name("world-reader".into()).spawn(move||loop {
            match reader.poll(){Ok(p)=>if tx.send(Ok(p)).is_err(){break},Err(error)=>{let _=tx.send(Err(error.to_string()));break}}
        })?;
        let autostart=config.task_file.as_ref().map(|p|->Result<TaskSpec>{Ok(serde_json::from_str(&std::fs::read_to_string(p).context("read task config")?)?)}).transpose()?;
        let now=Instant::now();let nav=Navigator::new(&config.mmaps);let strategy=crate::policy_runtime::PolicyRuntime::new(config.policy_file.clone())?;
        events.emit(None,"character.connected",json!({"character":client.character_name,"guid":client.character_guid,"class":client.view.class,"race":client.view.race}))?;
        Ok(Self{client,strategy,task:TaskEngine::default(),packets:rx,navigator:nav,leg:None,clock:now,last_move:now,last_heartbeat:now,last_status:now,last_ping:now,ping_sequence:0,events,public,config,last_group:vec![],autostart,shutdown:false,memory_at:now-Duration::from_secs(30),resident_kib:None,native_moving:false,position_revision:0})
    }
    fn emit(&self,kind:&str,detail:serde_json::Value)->Result<()>{self.events.emit(self.task.status().map(|s|s.id),kind,detail)?;Ok(())}
    fn stop(&mut self)->Result<()> {
        self.leg=None;
        if let Some(p)=self.client.view.position {
            self.client.writer.send_movement(opcode::MSG_MOVE_STOP,if self.client.view.rooted{0x1000}else{0},[p.x,p.y,p.z],p.orientation,0.,0,None,None)?;
        }
        Ok(())
    }
    fn navigate(&mut self,goal:Position)->Result<()> {
        let current=self.client.view.position.context("position_not_loaded")?;
        let owner=self.task.status();let task=owner.as_ref().map_or(0,|s|s.id);let phase=owner.map_or_else(String::new,|s|s.phase);
        if self.leg.as_ref().is_some_and(|l|l.task==task&&l.phase==phase&&l.goal.distance(&goal)<2.){return Ok(())}
        self.stop()?;
        let points=self.navigator.path(current,goal)?;
        let follower=Follower::new(points)?;
        self.emit("navigation.started",json!({"map":goal.map,"goal":goal}))?;
        self.leg=Some(Leg{follower,goal,task,phase});self.last_move=Instant::now();
        if !self.client.view.rooted&&!self.client.view.control_lost {
            self.client.writer.send_movement(opcode::MSG_MOVE_START_FORWARD,1,[current.x,current.y,current.z],current.orientation,0.,0,None,None)?;
        }
        Ok(())
    }
    fn action(&mut self,action:Action)->Result<()> {
        match action {
            Action::Navigate(goal)=>self.navigate(goal),
            Action::StopMoving=>self.stop(),
            Action::Cast{spell,target}=>{
                self.stop()?;
                if let Some(t)=target {if t!=self.client.character_guid{if let (Some(from),Some(to))=(self.client.view.position,self.client.view.objects.get(&t).and_then(|e|e.position)){let mut p=from;p.orientation=(to.y-from.y).atan2(to.x-from.x);self.client.writer.send_movement(opcode::MSG_MOVE_SET_FACING,if self.client.view.rooted{0x1000}else{0},[p.x,p.y,p.z],p.orientation,0.,0,None,None)?;self.client.view.position=Some(p);}}}
                self.client.execute(Action::Stand(0))?;
                self.client.execute(Action::Cast{spell,target})?;
                let(cast,_,gcd)=crate::strategy::spell_timing(spell);let now=self.client.view.now_ms;
                self.client.view.casting_until=now+cast.max(300);self.client.view.gcd_until=self.client.view.gcd_until.max(now+gcd);
                self.emit("action.cast_requested",json!({"spell":spell,"target":target}))
            }
            Action::UseItem{bag,slot}=>{self.client.execute(Action::UseItem{bag,slot})?;self.emit("action.item_use_requested",json!({"bag":bag,"slot":slot}))},
            other=>{self.client.execute(other)?;Ok(())}
        }
    }
    fn effects(&mut self,effects:Effects)->Result<()> {
        for(kind,detail)in effects.events{self.emit(&kind,detail)?;}
        for action in effects.actions {
            if let Err(error)=self.action(action.clone()) {
                let reason=error.to_string();self.emit("action.failed",json!({"reason":reason}))?;
                if matches!(action,Action::Navigate(_)){let e=self.task.event(GameEvent::NavigationFailed{reason},&self.client.view);for(kind,detail)in e.events{self.emit(&kind,detail)?;}}
            }
        }Ok(())
    }
    pub fn command(&mut self,command:Command)->Result<()> {
        let shutdown=matches!(command,Command::Shutdown);
        match self.task.command(command,&self.client.view){Ok(effects)=>self.effects(effects)?,Err(reason)=>self.emit("command.rejected",json!({"reason":reason}))?,}
        if shutdown{self.shutdown=true;}Ok(())
    }
    fn game_event(&mut self,event:GameEvent)->Result<()> {
        if matches!(event,GameEvent::DuelComplete{..}){self.strategy.combat_ended();}
        match event {
            GameEvent::GroupInvite{inviter}=>{
                self.emit("group.invite_received",json!({"inviter":inviter}))?;
                if self.config.auto_accept_group{self.client.execute(Action::AcceptGroup)?;self.emit("group.invite_accept_requested",json!({"mode":"continue_current_task"}))?;}
                Ok(())
            }
            GameEvent::Disconnected{reason}=>{self.client.view.connected=false;self.emit("agent.disconnected",json!({"reason":reason}))?;anyhow::bail!("world_disconnected")}
            other=>{let e=self.task.event(other,&self.client.view);self.effects(e)}
        }
    }
    fn movement(&mut self)->Result<()> {
        let elapsed=self.last_move.elapsed();if elapsed<Duration::from_millis(100){return Ok(())}self.last_move=Instant::now();
        if let Some(leg)=&self.leg {
            if self.task.status().is_some_and(|s|s.id!=leg.task||s.phase!=leg.phase||s.paused){self.stop()?;return Ok(())}
        }
        if self.native_moving||self.client.view.rooted||self.client.view.control_lost||self.client.view.now_ms<self.client.view.casting_until{return Ok(())}
        let Some(p)=self.client.view.position else{return Ok(())};
        let Some(leg)=&mut self.leg else{return Ok(())};
        match leg.follower.step(p,self.client.view.run_speed,elapsed) {
            Ok(Some(next))=>{
                self.client.writer.send_movement(opcode::MSG_MOVE_HEARTBEAT,1,[next.x,next.y,next.z],next.orientation,0.,0,None,None)?;
                // Ordinary clients own continuous local movement. Teleport/correction packets
                // overwrite this estimate; no instant relocation or speed override is used.
                self.client.view.position=Some(next);if let Some(me)=self.client.view.objects.get_mut(&self.client.character_guid){me.position=Some(next);}
                if leg.follower.is_done(){self.stop()?;self.emit("navigation.arrived",json!({"position":next}))?;let e=self.task.event(GameEvent::NavigationArrived,&self.client.view);self.effects(e)?;}
            }
            Ok(None)=>{self.stop()?;}
            Err(error)=>{let reason=error.to_string();self.stop()?;self.game_event(GameEvent::NavigationFailed{reason})?;}
        }Ok(())
    }
    fn update_status(&mut self)->Result<()> {
        if self.memory_at.elapsed()>=Duration::from_secs(30){self.memory_at=Instant::now();self.resident_kib=current_rss();self.emit("agent.memory_sample",json!({"resident_kib":self.resident_kib,"objects":self.client.view.objects.len()}))?;}
        let view=&self.client.view;let me=view.me();let task=self.task.status();
        let visible_objects=view.objects.values().filter(|e|matches!(e.kind,Kind::Player|Kind::Creature|Kind::GameObject)).count();
        let objects:Vec<_>=view.objects.values().filter(|e|matches!(e.kind,Kind::Player|Kind::Creature|Kind::GameObject)).cloned().take(512).collect();
        let mut public=self.public.lock().map_err(|_|anyhow::anyhow!("status_poisoned"))?;
        *public=Status{schema_version:1,connected:view.connected,character:view.character.clone(),class:view.class,position:view.position,task,health_pct:me.and_then(|e|e.health_pct()),mana_pct:me.and_then(|e|e.mana_pct()),visible_objects,event_sequence:self.events.history(0,1)?.latest,protocol_errors:self.client.protocol_errors,navigation:if self.leg.is_some(){"moving".into()}else{"idle".into()},instance_id:self.config.instance_id.clone(),account:self.config.account.username.clone(),race:view.race,group_members:view.group_members.clone(),resident_kib:self.resident_kib,known_spells:view.spells.len(),inventory_slots:view.inventory.len(),objects,policy:self.strategy.status()};Ok(())
    }
    pub fn run(mut self,commands:Receiver<Command>)->Result<()> {
        loop {
            self.client.view.now_ms=self.clock.elapsed().as_millis() as u64;
            for _ in 0..128 {match self.packets.try_recv(){Ok(Ok(p))=>{self.client.view.sequence+=1;let events=self.client.apply_poll(p)?;for event in events{self.game_event(event)?;}},Ok(Err(reason))=>{self.client.view.connected=false;self.emit("agent.disconnected",json!({"reason":reason}))?;self.update_status()?;anyhow::bail!("world_reader_closed")},Err(TryRecvError::Disconnected)=>{self.client.view.connected=false;self.update_status()?;anyhow::bail!("world_reader_closed")},Err(TryRecvError::Empty)=>break}}
            for _ in 0..32 {match commands.try_recv(){Ok(c)=>self.command(c)?,Err(_)=>break}}
            if self.shutdown{self.stop()?;self.client.writer.logout_request()?;self.client.view.connected=false;self.update_status()?;return Ok(())}
            if self.client.view.group_members!=self.last_group{self.last_group=self.client.view.group_members.clone();self.emit("group.roster_changed",json!({"members":self.last_group,"task_continues":true}))?;}
            if self.clock.elapsed()>Duration::from_secs(3)&&self.client.view.me().is_some()&&!self.client.view.spells.is_empty(){if let Some(task)=self.autostart.take(){self.command(Command::Start(task))?;}}
            if self.position_revision!=self.client.view.position_revision{self.position_revision=self.client.view.position_revision;if let Some(leg)=self.leg.take(){if let Err(error)=self.navigate(leg.goal){self.game_event(GameEvent::NavigationFailed{reason:error.to_string()})?;}}}
            let was_native=self.native_moving;self.native_moving=self.client.advance_paths(self.client.view.now_ms)?;
            if was_native&&!self.native_moving{if let Some(leg)=self.leg.take(){if let Err(error)=self.navigate(leg.goal){self.game_event(GameEvent::NavigationFailed{reason:error.to_string()})?;}}}
            let policy=&mut self.strategy;let effects=self.task.tick_with_strategy(&self.client.view,&mut |view,target|policy.choose(view,target).unwrap_or_else(||crate::strategy::choose_actions(view,target)));self.effects(effects)?;self.movement()?;
            if self.last_heartbeat.elapsed()>=Duration::from_secs(2){self.last_heartbeat=Instant::now();if self.leg.is_none()&&!self.native_moving&&!self.client.view.control_lost{if let Some(p)=self.client.view.position{self.client.writer.send_movement(opcode::MSG_MOVE_HEARTBEAT,if self.client.view.rooted{0x1000}else{0},[p.x,p.y,p.z],p.orientation,0.,0,None,None)?;}}}
            if self.last_ping.elapsed()>=Duration::from_secs(30){self.last_ping=Instant::now();self.ping_sequence=self.ping_sequence.wrapping_add(1);self.client.writer.ping(self.ping_sequence,0)?;}
            if self.last_status.elapsed()>=Duration::from_secs(1){self.last_status=Instant::now();self.update_status()?;}
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
fn current_rss()->Option<u64>{let output=std::process::Command::new("ps").args(["-p",&std::process::id().to_string(),"-o","rss="]).output().ok()?;std::str::from_utf8(&output.stdout).ok()?.trim().parse().ok()}
