//! First real benilla-protocol transport layer.
//!
//! This owns an authenticated world connection and translates a small set of authoritative
//! packets into agent events. It deliberately does not expose raw credentials or raw packets to
//! the HTTP API. More object-field mappings will be added incrementally.
use anyhow::{bail, Context, Result};
use benilla_protocol::{messages, events::{EntityKind, SessionEvent}, Poll, WorldReader, WorldSession, WorldWriter};
use crate::model::{Action,Entity,GameEvent,Kind,Position,WorldView};
use std::collections::BTreeMap;

#[derive(Clone)]
struct RemoteSpline{path:Vec<[f32;3]>,began:u64,offset:u64,duration:u64,id:u32,cyclic:bool,facing:messages::MonsterMoveFacing}
fn spline_point(path:&[[f32;3]],fraction:f32)->([f32;3],f32){
 let length=|a:[f32;3],b:[f32;3]|((a[0]-b[0]).powi(2)+(a[1]-b[1]).powi(2)+(a[2]-b[2]).powi(2)).sqrt();
 let total:f32=path.windows(2).map(|p|length(p[0],p[1])).sum();let mut left=total*fraction.clamp(0.,1.);
 for pair in path.windows(2){let d=length(pair[0],pair[1]);if d>0.&&left<=d{return(std::array::from_fn(|i|pair[0][i]+(pair[1][i]-pair[0][i])*left/d),(pair[1][1]-pair[0][1]).atan2(pair[1][0]-pair[0][0]));}left-=d;}
 (*path.last().unwrap_or(&[0.;3]),0.)
}
pub struct ConnectedClient {
    pub reader: Option<WorldReader>,
    pub writer: WorldWriter,
    pub character_guid: u64,
    pub character_name: String,
    pub view: WorldView,
    fields: BTreeMap<u64, messages::ObjectFields>,
    remote_splines:BTreeMap<u64,RemoteSpline>,
    staged:BTreeMap<u64,Entity>,
    pub protocol_errors:u64,
}

impl ConnectedClient {
    pub fn execute(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Select(guid) => self.writer.set_selection(guid)?,
            Action::Cast { spell, target } => self.writer.cast_spell(spell, target)?,
            Action::Attack(guid) => self.writer.attack_swing(guid)?,
            Action::AcceptGroup => self.writer.group_accept()?,
            Action::AcceptDuel(arbiter) => self.writer.duel_accepted(arbiter)?,
            Action::CancelDuel(arbiter) => self.writer.duel_cancelled(arbiter)?,
            Action::Stand(state) => self.writer.stand_state_change(state)?,
            Action::UseItem { bag, slot } => self.writer.use_item(bag, slot, 0, messages::UseItemTarget::SelfImplicit)?,
            Action::VendorList(vendor) => self.writer.list_inventory(vendor)?,
            Action::Buy { vendor, item, count, .. } => self.writer.buy_item(vendor, item, count)?,
            Action::StopAttack => self.writer.attack_stop()?,
            Action::Navigate(_) | Action::StopMoving => bail!("movement executor not connected yet"),
        }
        Ok(())
    }
    pub fn login(address: &str, username: &str, password: &str, character: &str) -> Result<Self> {
        let logon = benilla_protocol::logon(address, username, password, u32::from_be_bytes(*b"zhCN"))
            .context("logon failed")?;
        let realm = logon.realms.first().context("realmlist is empty")?;
        let mut session = WorldSession::connect(&realm.address, username, logon.session_key)
            .context("world auth failed")?;
        let roster = session.char_enum().context("character enumeration failed")?;
        let selected = roster.iter().find(|row| row.name.eq_ignore_ascii_case(character))
            .with_context(|| format!("character {character} was not found"))?;
        if !matches!(selected.class, 1 | 8) { bail!("only warrior and mage are supported") }
        let guid = selected.guid;
        let name = selected.name.clone();
        session.player_login(guid).context("player login failed")?;
        session.set_active_mover(guid).context("active mover setup failed")?;
        let (reader, writer) = session.into_split().context("world split failed")?;
        let mut view = WorldView::default();
        view.connected = true; view.self_guid = guid; view.character = name.clone();
        view.class = selected.class; view.race = selected.race; view.run_speed = 7.0; view.position=Some(Position{map:selected.map,x:selected.position.x,y:selected.position.y,z:selected.position.z,orientation:0.0});
        Ok(Self { reader:Some(reader), writer, character_guid: guid, character_name: name, view, fields: BTreeMap::new(), remote_splines:BTreeMap::new(), staged:BTreeMap::new(), protocol_errors:0 })
    }

    /// Poll one authoritative server packet. The protocol parser remains the source of truth.
    /// A group invite is accepted immediately and does not alter task state or send follow.
    pub fn poll_once(&mut self) -> Result<Vec<GameEvent>> {
        let packet=self.reader.as_mut().context("reader taken by runtime")?.poll().context("world reader poll failed")?;
        self.apply_poll(packet)
    }
    pub fn take_reader(&mut self)->Result<WorldReader>{self.reader.take().context("reader already taken")}
    pub fn apply_poll(&mut self, packet:Poll)->Result<Vec<GameEvent>>{
        let mut events=Vec::new();
        match packet {
            Poll::Events { events: decoded, .. } => {
                for decoded_event in decoded {
                match decoded_event {
                    SessionEvent::Worldport { map_id, position, orientation, needs_ack } => {
                        self.view.position_revision=self.view.position_revision.wrapping_add(1);
                        if needs_ack {self.writer.worldport_ack()?;self.fields.clear();self.view.objects.clear();self.remote_splines.clear();self.staged.clear();}
                        self.view.position=Some(Position{map:map_id,x:position[0],y:position[1],z:position[2],orientation});
                    }
                    SessionEvent::Teleport{guid,counter,position,orientation}=>{self.writer.teleport_ack(guid,counter)?;if guid==self.character_guid{self.view.position_revision=self.view.position_revision.wrapping_add(1);let map=self.view.position.map_or(0,|p|p.map);self.view.position=Some(Position{map,x:position[0],y:position[1],z:position[2],orientation});}}
                    SessionEvent::TransferPending{..}=>{self.view.position=None;self.view.objects.clear();self.fields.clear();}
                    SessionEvent::ObjectCreate { guid, kind, position, orientation, fields, speeds, spline, .. } => {
                        self.fields.insert(guid, fields.clone());
                        let entity = self.entity_from_fields(guid, kind, position, orientation, &fields);
                        if guid==self.character_guid { self.view.position=entity.position; }
                        self.view.objects.insert(guid, entity);
                        if guid==self.character_guid{if let Some(speeds)=speeds{self.view.run_speed=speeds.run;}}
                        if let Some(spline)=spline{if spline.path.len()>=2&&spline.duration_ms>0&&!spline.flying{self.remote_splines.insert(guid,RemoteSpline{path:spline.path,began:self.view.now_ms,offset:u64::from(spline.time_passed_ms),duration:u64::from(spline.duration_ms),id:spline.id,cyclic:spline.cyclic,facing:spline.facing});}}
                    }
                    SessionEvent::ItemCreate { guid, container, fields } => {
                        self.fields.insert(guid, fields);
                        self.view.objects.insert(guid, Entity { guid, kind: if container { Kind::Container } else { Kind::Item }, entry: 0, name: None, position: None, health: None, max_health: None, mana: None, max_mana: None, rage: None, level: None, class: None, flags: 0,duel_arbiter:0, auras: Default::default(), target: None, updated_ms: self.view.now_ms });
                    }
                    SessionEvent::ObjectValues { guid, fields } => {
                        let cache = self.fields.entry(guid).or_default(); cache.merge(fields);
                        let snapshot = cache.clone();
                        if let Some(entity) = self.view.objects.get_mut(&guid) { Self::update_entity(entity, &snapshot, self.view.now_ms); }
                    }
                    SessionEvent::ObjectMove { guid, position, orientation } | SessionEvent::UnitMove { guid, position, orientation, .. } => {
                        self.remote_splines.remove(&guid);
                        if let Some(entity)=self.view.objects.get_mut(&guid){entity.position=Some(Position{map:self.view.position.map_or(0,|p|p.map),x:position[0],y:position[1],z:position[2],orientation});entity.updated_ms=self.view.now_ms;}
                        if guid==self.character_guid { if let Some(old)=self.view.position {self.view.position=Some(Position{map:old.map,x:position[0],y:position[1],z:position[2],orientation});} }
                    }
                    SessionEvent::ObjectsRemoved(ids) => { for id in ids {self.remote_splines.remove(&id);if let Some(e)=self.view.objects.remove(&id){self.staged.insert(id,e);}}while self.staged.len()>4096{if let Some(id)=self.staged.keys().next().copied(){self.staged.remove(&id);self.fields.remove(&id);}} }
                    SessionEvent::ObjectsNear(ids) => {for id in ids{if let Some(e)=self.staged.remove(&id){self.view.objects.insert(id,e);}}}
                    SessionEvent::ObjectDestroyed(guid) => {self.remote_splines.remove(&guid); self.fields.remove(&guid); self.view.objects.remove(&guid); }
                    SessionEvent::ClientControl{mover,allow_move}=>{if mover==self.character_guid{self.view.control_lost=!allow_move;}}
                    SessionEvent::MonsterMove{guid,transport,start,path,spline_id,facing,stop,duration_ms,..}=>{
                        self.remote_splines.remove(&guid);
                        if transport.is_none(){if stop||duration_ms==0||path.len()<2{let map=self.view.position.map_or(0,|p|p.map);if let Some(e)=self.view.objects.get_mut(&guid){e.position=Some(Position{map,x:start[0],y:start[1],z:start[2],orientation:match facing{messages::MonsterMoveFacing::Angle(a)=>a,_=>e.position.map_or(0.,|p|p.orientation)}});}}else if self.remote_splines.len()<4096{self.remote_splines.insert(guid,RemoteSpline{path,began:self.view.now_ms,offset:0,duration:u64::from(duration_ms),id:spline_id,cyclic:false,facing});}}
                    }
                    SessionEvent::VendorInventory{vendor,items}=>{self.view.vendor=Some(crate::model::VendorView{guid:vendor,items:items.into_iter().filter_map(|i|u8::try_from(i.slot).ok().map(|slot|crate::model::VendorItem{slot,entry:i.entry,price:i.price,buy_count:i.buy_count})).collect()});}
                    SessionEvent::SpellStart{caster,spell_id,cast_time_ms,..}=>{let ends=self.view.now_ms+u64::from(cast_time_ms);self.view.casts.insert(caster,crate::model::CastView{spell:spell_id,ends_ms:ends});if caster==self.character_guid{events.push(GameEvent::SpellStarted{spell:spell_id,cast_ms:cast_time_ms});self.view.casting_until=ends;self.view.gcd_until=self.view.now_ms+crate::strategy::spell_timing(spell_id).2;}}
                    SessionEvent::SpellGo{caster,spell_id,hits,..}=>{if caster==self.character_guid{events.push(GameEvent::SpellLaunched{spell:spell_id,hits:hits.len()});}let requested=settle_cast(&mut self.view,caster,spell_id);if caster==self.character_guid&&requested{let(_,cd,gcd)=crate::strategy::spell_timing(spell_id);self.view.cooldowns.insert(spell_id,self.view.now_ms+cd);if crate::strategy::spell_timing(spell_id).0==0{self.view.gcd_until=self.view.gcd_until.max(self.view.now_ms+gcd);}}}
                    SessionEvent::SpellFailedOther{caster,spell_id}=>{settle_cast(&mut self.view,caster,spell_id);}
                    SessionEvent::SpellDelayed{caster,delay_ms}=>{if let Some(c)=self.view.casts.get_mut(&caster){c.ends_ms+=u64::from(delay_ms);}if caster==self.character_guid{self.view.casting_until+=u64::from(delay_ms);}}
                    SessionEvent::ChannelStart{duration_ms,..}=>{self.view.casting_until=self.view.now_ms+u64::from(duration_ms);}
                    SessionEvent::ChannelUpdate{remaining_ms}=>{self.view.casting_until=self.view.now_ms+u64::from(remaining_ms);}
                    SessionEvent::SpellDamageLog(log)=>{if log.attacker==self.character_guid||log.target==self.character_guid{events.push(GameEvent::Damage{spell:log.spell_id,source:log.attacker,target:log.target,amount:log.damage});}}
                    SessionEvent::AttackStart{attacker,victim}=>{if victim==self.character_guid{events.push(GameEvent::Attacked{attacker});}}
                    SessionEvent::ForceSpeedChange{guid,kind,counter,speed}=>{if guid==self.character_guid{if let Some(p)=self.view.position{self.writer.force_speed_change_ack(kind,guid,counter,speed,if self.view.rooted{0x1000}else{0},[p.x,p.y,p.z],p.orientation,0.,0,None,None)?;}if matches!(kind,messages::SpeedKind::Run){self.view.run_speed=speed;}}}
                    SessionEvent::MoveMode{guid,counter,mode,apply}=>{if guid==self.character_guid{if matches!(mode,messages::MoveMode::Root){self.view.rooted=apply;}if let Some(p)=self.view.position{self.writer.move_mode_ack(guid,counter,mode,apply,if self.view.rooted{0x1000}else{0},([p.x,p.y,p.z],p.orientation))?;}}}
                    SessionEvent::SpellBook { spell_ids, cooldowns } => { self.view.spells=spell_ids.into_iter().collect(); for c in cooldowns { self.view.cooldowns.insert(u32::from(c.spell_id), self.view.now_ms + u64::from(c.spell_cd_ms)); } }
                    SessionEvent::SpellLearned { spell_id } => { self.view.spells.insert(spell_id); }
                    SessionEvent::SpellRemoved { spell_id } => { self.view.spells.remove(&spell_id); }
                    SessionEvent::SpellSuperceded { old_spell_id, new_spell_id } => { self.view.spells.remove(&old_spell_id); self.view.spells.insert(new_spell_id); }
                    SessionEvent::SpellCooldowns { caster, cooldowns } if caster==self.character_guid => { for (id,ms) in cooldowns { self.view.cooldowns.insert(id,self.view.now_ms+u64::from(ms)); } }
                    SessionEvent::DuelCountdown { seconds } => {if let Some(d)=&mut self.view.duel{d.starts_ms=Some(self.view.now_ms+u64::from(seconds)*1000);d.active=true;}events.push(GameEvent::DuelCountdown { seconds });},
                    SessionEvent::DuelComplete { started } => {self.view.duel=None;events.push(GameEvent::DuelComplete { started });},
                    SessionEvent::DuelRequested { arbiter, challenger } => {self.view.duel=Some(crate::model::DuelView{arbiter,opponent:if challenger==self.character_guid{self.view.me().and_then(|m|m.target).unwrap_or(0)}else{challenger},starts_ms:None,active:false});events.push(GameEvent::DuelRequested { arbiter, challenger });},
                    SessionEvent::DuelWinner { winner, loser, .. } => events.push(GameEvent::DuelWinner { winner, loser }),
                    SessionEvent::CastResult { spell_id, success, reason, .. } if !success => {settle_cast(&mut self.view,self.character_guid,spell_id);events.push(GameEvent::CastFailed { spell: spell_id, reason: format!("server_result_{:?}", reason) });},
                    SessionEvent::Disconnected { reason, .. } => events.push(GameEvent::Disconnected { reason }),
                    SessionEvent::GroupList { members, .. } => { self.view.group_members=members.into_iter().map(|m|m.name).collect(); }
                    SessionEvent::GroupDestroyed | SessionEvent::GroupUninvited => {self.view.group_members.clear();}
                    SessionEvent::GroupInvite { .. } => { events.push(GameEvent::GroupInvite { inviter: 0 }); }
                    _ => {}
                }
                }

            }
            Poll::Skipped { .. } => {self.protocol_errors=self.protocol_errors.saturating_add(1);},
        }
        self.refresh_inventory();
        Ok(events)
    }

    pub fn apply_action(&mut self, action: Action) -> Result<()> { self.execute(action) }

    pub fn advance_paths(&mut self,now_ms:u64)->Result<bool>{
 let mut ended=Vec::new();let map=self.view.position.map_or(0,|p|p.map);
 for (&guid,spline) in &self.remote_splines{
  let elapsed=now_ms.saturating_sub(spline.began).saturating_add(spline.offset);let progress=if spline.cyclic{elapsed%spline.duration}else{elapsed.min(spline.duration)};
  let (xyz,mut orientation)=spline_point(&spline.path,progress as f32/spline.duration as f32);
  if progress>=spline.duration {match spline.facing{messages::MonsterMoveFacing::Angle(a)=>orientation=a,messages::MonsterMoveFacing::Spot(p)=>orientation=(p[1]-xyz[1]).atan2(p[0]-xyz[0]),messages::MonsterMoveFacing::Target(t)=>if let Some(p)=self.view.objects.get(&t).and_then(|e|e.position){orientation=(p.y-xyz[1]).atan2(p.x-xyz[0]);},_=>{}}}
  let p=Position{map,x:xyz[0],y:xyz[1],z:xyz[2],orientation};if let Some(entity)=self.view.objects.get_mut(&guid){entity.position=Some(p);entity.updated_ms=now_ms;}if guid==self.character_guid{self.view.position=Some(p);}
  if !spline.cyclic&&elapsed>=spline.duration{if guid==self.character_guid{self.writer.move_spline_done(0,xyz,orientation,spline.id)?;}ended.push(guid);}
 }
 for guid in ended{self.remote_splines.remove(&guid);}Ok(self.remote_splines.contains_key(&self.character_guid))
 }
    fn refresh_inventory(&mut self){
        if let Some(me)=self.fields.get(&self.character_guid){
            self.view.money=me.player_money();
            let mut items=Vec::new();
            for offset in 0u8..16 {let slot=23+offset;if let Some(guid)=me.player_pack_slot(offset){if let Some(f)=self.fields.get(&guid){if let Some(entry)=f.object_entry(){items.push(crate::model::InventoryItem{guid,entry,count:f.item_stack_count().unwrap_or(1),bag:255,slot});}}}}
            for bag in 19u8..23 {if let Some(bag_guid)=me.player_inv_slot(bag){if let Some(container)=self.fields.get(&bag_guid){for slot in 0u8..container.container_num_slots().unwrap_or(0).min(36) as u8 {if let Some(guid)=container.container_slot(slot){if let Some(f)=self.fields.get(&guid){if let Some(entry)=f.object_entry(){items.push(crate::model::InventoryItem{guid,entry,count:f.item_stack_count().unwrap_or(1),bag,slot});}}}}}}}
            self.view.inventory=items;
        }
    }
    fn entity_from_fields(&self,guid:u64,kind:EntityKind,pos:[f32;3],orientation:f32,fields:&messages::ObjectFields)->Entity{
        let kind=match kind{EntityKind::Player=>Kind::Player,EntityKind::Unit=>Kind::Creature,EntityKind::GameObject=>Kind::GameObject,EntityKind::Corpse|EntityKind::DynamicObject|EntityKind::Other=>Kind::Other};
        let mut e=Entity{guid,kind,entry:0,name:None,position:Some(Position{map:self.view.position.map_or(0,|p|p.map),x:pos[0],y:pos[1],z:pos[2],orientation}),health:None,max_health:None,mana:None,max_mana:None,rage:None,level:None,class:None,flags:0,duel_arbiter:0,auras:Default::default(),target:None,updated_ms:self.view.now_ms}; Self::update_entity(&mut e,fields,self.view.now_ms); e
    }
    fn update_entity(e:&mut Entity,f:&messages::ObjectFields,now:u64){e.entry=f.object_entry().unwrap_or(e.entry);e.health=f.unit_health();e.max_health=f.unit_max_health();e.mana=f.unit_power(0);e.max_mana=f.unit_max_power(0);e.rage=f.unit_shown_power(1);e.auras=f.unit_aura_ids().filter(|x|*x!=0).collect();e.level=f.unit_level().and_then(|x|u8::try_from(x).ok());e.flags= f.unit_flags();e.class=f.unit_class();e.duel_arbiter=if e.kind==Kind::Player{f.player_duel_arbiter()}else{0};e.target=f.unit_target();e.updated_ms=now;}
}

// Triggered effects (e.g. Frost Armor chill) launch while a real cast is still preparing.
// Only the matching requested spell may finish that cast; never clear it for every SpellGo.
fn settle_cast(view:&mut WorldView,caster:u64,spell:u32)->bool{
    if view.casts.get(&caster).is_some_and(|c|c.spell==spell){view.casts.remove(&caster);}
    if caster==view.self_guid&&view.pending_spell==Some(spell){view.pending_spell=None;view.casting_until=view.now_ms;return true;}
    false
}
#[cfg(test)]mod spline_tests{
 use super::*;
 #[test]fn triggered_chill_does_not_cancel_frostbolt_cast(){let mut view=WorldView{self_guid:223,now_ms:1000,pending_spell:Some(25304),casting_until:4000,..Default::default()};view.casts.insert(223,crate::model::CastView{spell:25304,ends_ms:4000});assert!(!settle_cast(&mut view,223,7321));assert_eq!(view.casting_until,4000);assert_eq!(view.pending_spell,Some(25304));assert_eq!(view.casts[&223].spell,25304);assert!(settle_cast(&mut view,223,25304));assert_eq!(view.casting_until,1000);assert_eq!(view.pending_spell,None);}
 #[test]fn unrelated_failure_does_not_clear_active_cast(){let mut view=WorldView{self_guid:223,now_ms:1000,pending_spell:Some(25304),casting_until:4000,..Default::default()};assert!(!settle_cast(&mut view,223,1131));assert_eq!(view.casting_until,4000);}
 #[test]fn arc_length_not_point_index(){let path=[[0.,0.,0.],[1.,0.,0.],[1.,9.,0.]];let(p,_)=spline_point(&path,0.5);assert!((p[0]-1.).abs()<0.001&&(p[1]-4.).abs()<0.001);let(p,_)=spline_point(&path,1.);assert_eq!(p,[1.,9.,0.]);}
 #[test]fn height_and_duplicate_points(){let path=[[0.,0.,0.],[0.,0.,0.],[0.,0.,10.]];assert_eq!(spline_point(&path,0.5).0,[0.,0.,5.]);}
}
