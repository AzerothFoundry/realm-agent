use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub map: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    #[serde(default)]
    pub orientation: f32,
}
impl Position {
    pub fn distance(&self, other: &Self) -> f32 {
        if self.map != other.map { return f32::INFINITY; }
        ((self.x-other.x).powi(2)+(self.y-other.y).powi(2)+(self.z-other.z).powi(2)).sqrt()
    }
    pub fn finite(&self) -> bool { [self.x,self.y,self.z,self.orientation].iter().all(|v|v.is_finite()) }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum Kind { Player, Creature, GameObject, Item, Container, Other }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    pub guid: u64,
    pub kind: Kind,
    pub entry: u32,
    pub name: Option<String>,
    pub position: Option<Position>,
    pub health: Option<u32>,
    pub max_health: Option<u32>,
    pub mana: Option<u32>,
    pub max_mana: Option<u32>,
    #[serde(default)] pub rage: Option<u32>,
    pub level: Option<u8>,
    pub class: Option<u8>,
    pub flags: u32,
    pub auras: BTreeSet<u32>,
    pub target: Option<u64>,
    pub updated_ms: u64,
}
impl Entity {
    pub fn health_pct(&self) -> Option<f32> { self.health.zip(self.max_health).filter(|(_,m)|*m>0).map(|(h,m)|100.0*h as f32/m as f32) }
    pub fn mana_pct(&self) -> Option<f32> { self.mana.zip(self.max_mana).filter(|(_,m)|*m>0).map(|(h,m)|100.0*h as f32/m as f32) }
    pub fn alive(&self) -> bool { self.health.is_some_and(|h|h>0) }
    pub fn in_combat(&self) -> bool { self.flags & 0x00080000 != 0 }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InventoryItem {
    pub guid: u64, pub entry: u32, pub count: u32, pub bag: u8, pub slot: u8,
}
#[derive(Clone, Debug, Default)]
pub struct WorldView {
    pub now_ms: u64, pub sequence: u64, pub connected: bool,
    pub self_guid: u64, pub character: String, pub race: u8, pub class: u8,
    pub position: Option<Position>, pub objects: BTreeMap<u64,Entity>,
    pub spells: BTreeSet<u32>, pub cooldowns: BTreeMap<u32,u64>,
    pub casting_until: u64, pub gcd_until: u64, pub inventory: Vec<InventoryItem>,
    pub money: Option<u32>, pub vendor: Option<VendorView>,
    pub duel: Option<DuelView>, pub run_speed: f32,
    pub casts: BTreeMap<u64, CastView>,
}
impl WorldView {
    pub fn me(&self) -> Option<&Entity> { self.objects.get(&self.self_guid) }
    pub fn ready(&self, spell:u32) -> bool { self.spells.contains(&spell) && self.now_ms>=self.cooldowns.get(&spell).copied().unwrap_or(0) && self.now_ms>=self.gcd_until && self.now_ms>=self.casting_until }
    pub fn count(&self, ids:&[u32]) -> u32 { self.inventory.iter().filter(|i|ids.contains(&i.entry)).map(|i|i.count).sum() }
}
#[derive(Clone, Debug, Default)]
pub struct DuelView { pub arbiter:u64, pub opponent:u64, pub starts_ms:Option<u64>, pub active:bool }
#[derive(Clone, Debug, Default)]
pub struct CastView { pub spell:u32, pub ends_ms:u64 }
#[derive(Clone, Debug, Default)]
pub struct VendorView { pub guid:u64, pub items:Vec<VendorItem> }
#[derive(Clone, Debug, Default)]
pub struct VendorItem { pub slot:u8, pub entry:u32, pub price:u32, pub buy_count:u32 }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub inn: Position, pub arena: Position, pub radius:f32,
    pub vendor_entry:u32, pub food_entry:u32,
    #[serde(default)] pub teleport_spell:Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default="default_task")] pub kind:String,
    pub site:Site,
    pub opponents:Vec<u64>,
    #[serde(default="default_duration")] pub duration_secs:u64,
    #[serde(default="default_supply")] pub supply_count:u32,
    #[serde(default)] pub max_duels:Option<u32>,
}
fn default_schema_version()->u32{1}
fn default_task()->String{"city_duel".into()}
fn default_duration()->u64{3600}
fn default_supply()->u32{20}

#[derive(Clone, Debug)]
pub enum Command { Start(TaskSpec), Pause, Resume, Cancel, Shutdown }
#[derive(Clone, Debug)]
pub enum GameEvent {
    DuelRequested{arbiter:u64,challenger:u64}, DuelCountdown{seconds:u32},
    DuelComplete{started:bool}, DuelWinner{winner:String,loser:String},
    Attacked{attacker:u64}, CastFailed{spell:u32,reason:String}, GroupInvite{inviter:u64},
    Disconnected{reason:String}, NavigationArrived, NavigationFailed{reason:String},
}
#[derive(Clone, Debug)]
pub enum Action {
    Select(u64), Cast{spell:u32,target:Option<u64>}, AcceptDuel(u64), CancelDuel(u64),
    Navigate(Position), StopMoving, Stand(u32), UseItem{bag:u8,slot:u8},
    VendorList(u64), Buy{vendor:u64,item:u32,slot:u8,count:u8}, StopAttack,
    Attack(u64), AcceptGroup,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct TaskStatus {
    pub id:u64, pub kind:String, pub phase:String, pub reason:String,
    pub opponent:Option<u64>, pub started_ms:u64, pub deadline_ms:u64,
    pub duels:u32, pub wins:u32, pub losses:u32, pub paused:bool,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub schema_version:u32, pub connected:bool, pub character:String,
    pub class:u8, pub position:Option<Position>, pub task:Option<TaskStatus>,
    pub health_pct:Option<f32>, pub mana_pct:Option<f32>, pub visible_objects:usize,
    pub event_sequence:u64, pub protocol_errors:u64, pub navigation:String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentEvent {
    pub schema_version:u32, pub id:u64, pub time_ms:u64, pub task_id:Option<u64>,
    pub kind:String, pub detail:serde_json::Value,
}
#[derive(Clone, Debug, Default)]
pub struct Effects { pub actions:Vec<Action>, pub events:Vec<(String,serde_json::Value)> }
