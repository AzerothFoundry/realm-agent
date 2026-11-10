//! First real benilla-protocol transport layer.
//!
//! This owns an authenticated world connection and translates a small set of authoritative
//! packets into agent events. It deliberately does not expose raw credentials or raw packets to
//! the HTTP API. More object-field mappings will be added incrementally.
use anyhow::{bail, Context, Result};
use benilla_protocol::{messages::{self, ServerPacket}, Poll, WorldReader, WorldSession, WorldWriter};
use crate::model::{GameEvent, WorldView};

pub struct ConnectedClient {
    pub reader: WorldReader,
    pub writer: WorldWriter,
    pub character_guid: u64,
    pub character_name: String,
    pub view: WorldView,
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
        Ok(Self { reader, writer, character_guid: guid, character_name: name, view })
    }

    /// Poll one authoritative server packet. The protocol parser remains the source of truth.
    /// A group invite is accepted immediately and does not alter task state or send follow.
    pub fn poll_once(&mut self) -> Result<Vec<GameEvent>> {
        let mut events = Vec::new();
        match self.reader.poll().context("world reader poll failed")? {
            Poll::Events { opcode, body, .. } => {
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
            }
            Poll::Skipped { reason, .. } => events.push(GameEvent::Disconnected { reason }),
        }
        Ok(events)
    }
}
