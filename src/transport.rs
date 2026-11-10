//! First real benilla-protocol transport layer.
//!
//! This owns an authenticated world connection and translates a small set of authoritative
//! packets into agent events. It deliberately does not expose raw credentials or raw packets to
//! the HTTP API. More object-field mappings will be added incrementally.
use anyhow::{bail, Context, Result};
use benilla_protocol::{messages, events::{EntityKind, SessionEvent}, Poll, WorldReader, WorldSession, WorldWriter};
use crate::model::{Entity,GameEvent,Kind,Position,WorldView};
use std::collections::BTreeMap;

pub struct ConnectedClient {
    pub reader: WorldReader,
    pub writer: WorldWriter,
    pub character_guid: u64,
    pub character_name: String,
    pub view: WorldView,
    fields: BTreeMap<u64, messages::ObjectFields>,
}

impl ConnectedClient {
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
        view.class = selected.class; view.race = selected.race; view.run_speed = 7.0;
        Ok(Self { reader, writer, character_guid: guid, character_name: name, view, fields: BTreeMap::new() })
    }

    /// Poll one authoritative server packet. The protocol parser remains the source of truth.
    /// A group invite is accepted immediately and does not alter task state or send follow.
    pub fn poll_once(&mut self) -> Result<Vec<GameEvent>> {
        let mut events = Vec::new();
        match self.reader.poll().context("world reader poll failed")? {
            Poll::Events { events: decoded, .. } => {
                for decoded_event in decoded {
                match decoded_event {
                    SessionEvent::ObjectCreate { guid, kind, position, orientation, fields, .. } => {
                        self.fields.insert(guid, fields.clone());
                        let entity = self.entity_from_fields(guid, kind, position, orientation, &fields);
                        self.view.objects.insert(guid, entity);
                    }
                    SessionEvent::ItemCreate { guid, container, fields } => {
                        self.fields.insert(guid, fields);
                        self.view.objects.insert(guid, Entity { guid, kind: if container { Kind::Container } else { Kind::Item }, entry: 0, name: None, position: None, health: None, max_health: None, mana: None, max_mana: None, rage: None, level: None, class: None, flags: 0, auras: Default::default(), target: None, updated_ms: self.view.now_ms });
                    }
                    SessionEvent::ObjectValues { guid, fields } => {
                        let cache = self.fields.entry(guid).or_default(); cache.merge(fields);
                        let snapshot = cache.clone();
                        if let Some(entity) = self.view.objects.get_mut(&guid) { Self::update_entity(entity, &snapshot, self.view.now_ms); }
                    }
                    SessionEvent::ObjectMove { guid, position, orientation } | SessionEvent::UnitMove { guid, position, orientation, .. } => {
                        if let Some(entity)=self.view.objects.get_mut(&guid){entity.position=Some(Position{map:self.view.position.map_or(0,|p|p.map),x:position[0],y:position[1],z:position[2],orientation});entity.updated_ms=self.view.now_ms;}
                        if guid==self.character_guid { if let Some(old)=self.view.position {self.view.position=Some(Position{map:old.map,x:position[0],y:position[1],z:position[2],orientation});} }
                    }
                    SessionEvent::ObjectsRemoved(ids) | SessionEvent::ObjectsNear(ids) => { let _=ids; }
                    SessionEvent::ObjectDestroyed(guid) => { self.fields.remove(&guid); self.view.objects.remove(&guid); }
                    SessionEvent::SpellBook { spell_ids, cooldowns } => { self.view.spells=spell_ids.into_iter().collect(); for c in cooldowns { self.view.cooldowns.insert(u32::from(c.spell_id), self.view.now_ms + u64::from(c.spell_cd_ms)); } }
                    SessionEvent::SpellLearned { spell_id } => { self.view.spells.insert(spell_id); }
                    SessionEvent::SpellRemoved { spell_id } => { self.view.spells.remove(&spell_id); }
                    SessionEvent::SpellSuperceded { old_spell_id, new_spell_id } => { self.view.spells.remove(&old_spell_id); self.view.spells.insert(new_spell_id); }
                    SessionEvent::SpellCooldowns { caster, cooldowns } if caster==self.character_guid => { for (id,ms) in cooldowns { self.view.cooldowns.insert(id,self.view.now_ms+u64::from(ms)); } }
                    SessionEvent::DuelCountdown { seconds } => events.push(GameEvent::DuelCountdown { seconds }),
                    SessionEvent::DuelComplete { started } => events.push(GameEvent::DuelComplete { started }),
                    SessionEvent::DuelRequested { arbiter, challenger } => events.push(GameEvent::DuelRequested { arbiter, challenger }),
                    SessionEvent::DuelWinner { winner, loser, .. } => events.push(GameEvent::DuelWinner { winner, loser }),
                    SessionEvent::CastResult { spell_id, success, reason, .. } if !success => events.push(GameEvent::CastFailed { spell: spell_id, reason: format!("server_result_{:?}", reason) }),
                    SessionEvent::Disconnected { reason, .. } => events.push(GameEvent::Disconnected { reason }),
                    SessionEvent::GroupInvite { .. } => { self.writer.group_accept()?; events.push(GameEvent::GroupInvite { inviter: 0 }); }
                    _ => {}
                }
                }
                /*
                match messages::parse_server(opcode, &body) {
                    Ok(ServerPacket::NewWorld { map, position, orientation }) => {
                        self.writer.worldport_ack()?;
                        self.view.position = Some(crate::model::Position { map, x: position.x, y: position.y, z: position.z, orientation });
                    }
                    Ok(ServerPacket::LoginVerifyWorld { map, position, orientation }) => {
                        self.view.position = Some(crate::model::Position { map, x: position.x, y: position.y, z: position.z, orientation });
                    }
                    Ok(ServerPacket::Teleport { guid, counter, position, orientation }) => {
                        self.writer.teleport_ack(guid, counter)?;
                        if guid == self.character_guid { if let Some(old) = self.view.position { self.view.position = Some(crate::model::Position { map: old.map, x: position.x, y: position.y, z: position.z, orientation }); } }
                    }
                    Ok(ServerPacket::GroupInvite { inviter }) => {
                        self.writer.group_accept()?;
                        events.push(GameEvent::GroupInvite { inviter: 0 });
                    }
                    Ok(ServerPacket::DuelCountdown { seconds }) => events.push(GameEvent::DuelCountdown { seconds }),
                    Ok(ServerPacket::DuelComplete { started }) => events.push(GameEvent::DuelComplete { started }),
                    Ok(ServerPacket::DuelRequested { arbiter, challenger }) => events.push(GameEvent::DuelRequested { arbiter, challenger }),
                    Ok(ServerPacket::DuelWinner { winner, loser, .. }) => events.push(GameEvent::DuelWinner { winner, loser }),
                    Ok(ServerPacket::CastResult { spell_id, outcome }) => {
                        if !matches!(outcome, messages::CastOutcome::Ok) { events.push(GameEvent::CastFailed { spell: spell_id, reason: format!("server_result_{outcome:?}" )}); }
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
                */
            }
            Poll::Skipped { reason, .. } => events.push(GameEvent::Disconnected { reason }),
        }
        Ok(events)
    }

    fn entity_from_fields(&self,guid:u64,kind:EntityKind,pos:[f32;3],orientation:f32,fields:&messages::ObjectFields)->Entity{
        let kind=match kind{EntityKind::Player=>Kind::Player,EntityKind::Unit=>Kind::Creature,EntityKind::GameObject=>Kind::GameObject,EntityKind::Corpse|EntityKind::DynamicObject|EntityKind::Other=>Kind::Other};
        let mut e=Entity{guid,kind,entry:0,name:None,position:Some(Position{map:self.view.position.map_or(0,|p|p.map),x:pos[0],y:pos[1],z:pos[2],orientation}),health:None,max_health:None,mana:None,max_mana:None,rage:None,level:None,class:None,flags:0,auras:Default::default(),target:None,updated_ms:self.view.now_ms}; Self::update_entity(&mut e,fields,self.view.now_ms); e
    }
    fn update_entity(e:&mut Entity,f:&messages::ObjectFields,now:u64){e.health=f.unit_health();e.level=f.unit_level().and_then(|x|u8::try_from(x).ok());e.flags= f.unit_flags();e.target=f.unit_target();e.updated_ms=now;}
}
