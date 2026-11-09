//! Client-owned city duel lifecycle. All effects are ordinary client actions.
use crate::model::*;
use serde_json::json;

pub const FOOD: &[u32] = &[5349,1113,1114,1487,8075,8076,22895,8952,8950];
pub const WATER: &[u32] = &[5350,2288,2136,3772,8077,8078,8079];
const FOOD_SPELLS: &[u32] = &[587,597,990,6129,10144,10145];
const WATER_SPELLS: &[u32] = &[5504,5505,5506,6127,10138,10139,10140];
fn highest(w:&WorldView,ids:&[u32])->Option<u32>{ids.iter().rev().find(|s|w.spells.contains(s)).copied()}
fn full(w:&WorldView)->bool{w.me().is_some_and(|p|p.alive()&&p.health_pct().is_some_and(|h|h>=99.)&&(w.class!=8||p.mana_pct().is_some_and(|m|m>=99.)))}
#[derive(Default)]
pub struct TaskEngine {spec:Option<TaskSpec>,status:Option<TaskStatus>,next_id:u64,next_action:u64,phase_at:u64,nav_pending:bool,arbiter:u64,combat_seen:bool,passive:bool,countdown_until:u64,last_food:u64,last_water:u64,buff_tried:Vec<u32>}
impl TaskEngine {
 pub fn status(&self)->Option<TaskStatus>{self.status.clone()}
 fn phase(&mut self,p:&str,reason:&str,w:&WorldView,e:&mut Effects){if let Some(s)=&mut self.status{if s.phase!=p||s.reason!=reason{e.events.push(("task.phase_changed".into(),json!({"from":s.phase,"phase":p,"reason":reason})));s.phase=p.into();s.reason=reason.into();self.phase_at=w.now_ms;self.nav_pending=false;}}}
 fn cleanup(&mut self,e:&mut Effects){if self.arbiter!=0{e.actions.push(Action::CancelDuel(self.arbiter));self.arbiter=0;}e.actions.extend([Action::StopMoving,Action::StopAttack]);self.nav_pending=false;}
 pub fn command(&mut self,c:Command,w:&WorldView)->Result<Effects,String>{let mut e=Effects::default();match c{
 Command::Start(spec)=>{
 if !matches!(w.class,1|8){return Err("only mage and warrior supported".into());}
 if !w.connected{return Err("not connected".into());}
 if self.status.as_ref().is_some_and(|s|!matches!(s.phase.as_str(),"completed"|"cancelled"|"failed")){return Err("task already active".into());}
 if spec.kind!="city_duel"||!(60..=86400).contains(&spec.duration_secs)||!(1..=100).contains(&spec.supply_count)||!spec.site.inn.finite()||!spec.site.arena.finite()||spec.site.inn.map!=spec.site.arena.map||!spec.site.radius.is_finite()||!(10.0..=80.0).contains(&spec.site.radius)||spec.opponents.len()>256||spec.opponents.contains(&0)||spec.opponents.contains(&w.self_guid){return Err("invalid task specification".into());}
 self.next_id+=1;self.status=Some(TaskStatus{id:self.next_id,kind:spec.kind.clone(),phase:"checking_supplies".into(),reason:"task_started".into(),started_ms:w.now_ms,deadline_ms:w.now_ms.saturating_add(spec.duration_secs*1000),..Default::default()});self.countdown_until=0;self.spec=Some(spec);self.next_action=0;self.phase_at=w.now_ms;self.nav_pending=false;self.buff_tried.clear();self.combat_seen=false;self.arbiter=0;e.events.push(("task.started".into(),json!({"id":self.next_id})));},
 Command::Pause=>{self.cleanup(&mut e);if let Some(s)=&mut self.status{s.paused=true;e.events.push(("task.paused".into(),json!({})));}},
 Command::Resume=>{if let Some(s)=&mut self.status{s.paused=false;}self.phase("checking_supplies","resumed",w,&mut e);},
 Command::Cancel|Command::Shutdown=>{self.cleanup(&mut e);self.phase("cancelled","requested",w,&mut e);e.events.push(("task.cancelled".into(),json!({})));}
 }Ok(e)}
 pub fn event(&mut self,event:GameEvent,w:&WorldView)->Effects{let mut e=Effects::default();let Some(spec)=self.spec.clone()else{return e};let Some(s)=self.status.as_ref()else{return e};if s.paused||matches!(s.phase.as_str(),"completed"|"cancelled"|"failed"){return e;}
 match event{
 GameEvent::NavigationArrived=>{self.nav_pending=false;let p=s.phase.clone();match p.as_str(){"travelling_inn"=>self.phase("buying_supplies","inn_arrived",w,&mut e),"travelling_arena"=>self.phase("preparing","arena_arrived",w,&mut e),"approaching"=>self.phase("challenging","opponent_reached",w,&mut e),_=>{}}},
 GameEvent::NavigationFailed{reason}=>{self.nav_pending=false;self.phase("blocked",&format!("navigation:{reason}"),w,&mut e);e.events.push(("navigation.failed".into(),json!({"reason":reason})));},
 GameEvent::DuelRequested{arbiter,challenger}=>{
 let opponent=if challenger==w.self_guid{s.opponent}else{Some(challenger)};
 if opponent.is_some_and(|id|spec.opponents.contains(&id))&&full(w)&&w.me().is_some_and(|m|!m.in_combat()){
 self.arbiter=arbiter;if let Some(st)=&mut self.status{st.opponent=opponent;}if let Some(id)=opponent{e.actions.push(Action::Select(id));}e.actions.push(Action::AcceptDuel(arbiter));self.phase("countdown","challenge_accepted",w,&mut e);
 }else{e.actions.push(Action::CancelDuel(arbiter));e.events.push(("duel.rejected".into(),json!({"reason":"not_ready_or_not_allowlisted"})));}},
 GameEvent::DuelCountdown{seconds}=>{if self.arbiter!=0{self.countdown_until=w.now_ms+u64::from(seconds)*1000;self.phase("countdown","server_countdown",w,&mut e);}},
 GameEvent::DuelComplete{started}=>{self.arbiter=0;e.actions.extend([Action::StopAttack,Action::StopMoving]);if started{if let Some(st)=&mut self.status{st.duels+=1;}}self.combat_seen=false;self.buff_tried.clear();self.phase("recovering","duel_finished",w,&mut e);e.events.push(("combat.ended".into(),json!({"duel":true,"started":started})));},
 GameEvent::DuelWinner{winner,loser}=>{if let Some(st)=&mut self.status{if winner==w.character{st.wins+=1;}if loser==w.character{st.losses+=1;}}},
 GameEvent::GroupInvite{inviter}=>{e.actions.push(Action::AcceptGroup);e.events.push(("group.invite_accepted".into(),json!({"inviter":inviter,"mode":"continue_current_task"})));},
 GameEvent::Attacked{attacker}=>{e.events.push(("enemy.encountered".into(),json!({"guid":attacker,"source":"attack"})));if !self.combat_seen{self.passive=self.arbiter==0;self.combat_seen=true;e.events.push(("combat.started".into(),json!({"mode":if self.passive{"passive"}else{"duel"},"attacker":attacker})));}},
 GameEvent::CastFailed{spell,reason}=>{self.next_action=w.now_ms+1500;e.events.push(("action.rejected".into(),json!({"spell":spell,"reason":reason})));},
 GameEvent::Disconnected{reason}=>{self.cleanup(&mut e);self.phase("failed","disconnected",w,&mut e);e.events.push(("task.failed".into(),json!({"reason":reason})));}
 }e}
 fn navigate(&mut self,p:Position,e:&mut Effects){if !self.nav_pending{self.nav_pending=true;e.actions.push(Action::Navigate(p));}}
 pub fn tick(&mut self,w:&WorldView)->Effects{
 let mut e=Effects::default();let Some(spec)=self.spec.clone()else{return e};let Some(s)=self.status.clone()else{return e};if s.paused||matches!(s.phase.as_str(),"completed"|"cancelled"|"failed"){return e;}
 if w.now_ms>=s.deadline_ms||spec.max_duels.is_some_and(|n|s.duels>=n){self.cleanup(&mut e);self.phase("completed","limit_reached",w,&mut e);e.events.push(("task.completed".into(),json!({"duels":s.duels})));return e;}
 let Some(me)=w.me()else{return e};if !me.alive(){self.phase("blocked","dead_or_unknown_health",w,&mut e);return e;}
 if me.in_combat()&&self.arbiter==0{if !self.combat_seen{self.combat_seen=true;self.passive=true;e.events.push(("combat.started".into(),json!({"mode":"passive"})));}if let Some(target)=me.target.filter(|id|spec.opponents.contains(id)){if let Some(a)=crate::strategy::choose(w,target){e.actions.push(a);}}return e;}
 if self.passive&&self.combat_seen&&!me.in_combat(){self.combat_seen=false;self.passive=false;e.events.push(("combat.ended".into(),json!({"mode":"passive"})));self.phase("recovering","combat_ended",w,&mut e);}
 if w.now_ms<self.next_action{return e;}self.next_action=w.now_ms+500;
 let food=w.inventory.iter().filter(|i|FOOD.contains(&i.entry)||i.entry==spec.site.food_entry).map(|i|i.count).sum::<u32>();let water=w.count(WATER);
 match s.phase.as_str(){
 "checking_supplies"=>{if food>=spec.supply_count&&(w.class!=8||water>=spec.supply_count){self.phase("travelling_arena","supplies_ready",w,&mut e);}else if w.class==8{self.phase("conjuring","supplies_low",w,&mut e);}else{self.phase("travelling_inn","food_low",w,&mut e);}},
 "conjuring"=>{
 let spell=if food<spec.supply_count{highest(w,FOOD_SPELLS)}else if water<spec.supply_count{highest(w,WATER_SPELLS)}else{self.phase("travelling_arena","conjured_supplies_confirmed",w,&mut e);None};
 if let Some(spell)=spell{if w.ready(spell){e.actions.extend([Action::Stand(0),Action::Cast{spell,target:None}]);self.next_action=w.now_ms+4500;}}else if self.status.as_ref().is_some_and(|s|s.phase=="conjuring"){self.phase("travelling_inn","conjure_unavailable",w,&mut e);}
 if w.now_ms.saturating_sub(self.phase_at)>90000{self.phase("blocked","conjuring_timeout_or_inventory_full",w,&mut e);}
 },
 "travelling_inn"=>{if let Some(p)=w.position{if p.map!=spec.site.inn.map{if let Some(spell)=spec.site.teleport_spell.filter(|id|w.ready(*id)&&w.count(&[17031])>0){e.actions.push(Action::Cast{spell,target:None});self.next_action=w.now_ms+15000;}else{self.phase("blocked","no_cross_map_travel_capability",w,&mut e);}}else if p.distance(&spec.site.inn)<=5.{self.phase("buying_supplies","inn_reached",w,&mut e);}else{self.navigate(spec.site.inn,&mut e);}}},
 "buying_supplies"=>{if food>=spec.supply_count&&(w.class!=8||water>=spec.supply_count){self.phase("travelling_arena","purchase_confirmed",w,&mut e);}else if let Some(v)=w.objects.values().find(|v|v.entry==spec.site.vendor_entry&&v.alive()&&v.position.zip(w.position).is_some_and(|(a,b)|a.distance(&b)<=6.)){
 if let Some(shop)=w.vendor.as_ref().filter(|shop|shop.guid==v.guid){let item=if food<spec.supply_count{shop.items.iter().find(|i|i.entry==spec.site.food_entry)}else{shop.items.iter().find(|i|WATER.contains(&i.entry))};if let Some(i)=item{if w.money.is_some_and(|gold|gold>=i.price){e.actions.push(Action::Buy{vendor:v.guid,item:i.entry,slot:i.slot,count:1});self.next_action=w.now_ms+1500;}else{self.phase("blocked","insufficient_money",w,&mut e);}}else{self.phase("blocked","vendor_missing_supply",w,&mut e);}}else{e.actions.push(Action::VendorList(v.guid));self.next_action=w.now_ms+2000;}
 }else{self.phase("blocked","vendor_not_visible_or_out_of_range",w,&mut e);}if w.now_ms.saturating_sub(self.phase_at)>60000{self.phase("blocked","purchase_timeout",w,&mut e);}},
 "travelling_arena"=>{if let Some(p)=w.position{if p.distance(&spec.site.arena)<=spec.site.radius*0.5{self.phase("preparing","arena_reached",w,&mut e);}else if p.map==spec.site.arena.map{self.navigate(spec.site.arena,&mut e);}else{self.phase("travelling_inn","return_to_task_continent",w,&mut e);}}},
 "preparing"=>{if !full(w){self.phase("recovering","not_recovered",w,&mut e);}else{let choices=if w.class==8{vec![highest(w,&[1459,1460,1461,10156,10157]),highest(w,&[168,7300,7301,7302,7320,10219,10220]),highest(w,&[11426,13031,13032,13033])]}else{vec![]};if let Some(spell)=choices.into_iter().flatten().find(|id|!me.auras.contains(id)&&!self.buff_tried.contains(id)&&w.ready(*id)){self.buff_tried.push(spell);e.actions.push(Action::Cast{spell,target:None});self.next_action=w.now_ms+2000;}else{self.phase("seeking","prepared",w,&mut e);}}},
 "seeking"=>{if food==0||(w.class==8&&water==0){self.phase("checking_supplies","supplies_exhausted",w,&mut e);}else if let Some(pos)=w.position{if let Some(opponent)=spec.opponents.iter().filter_map(|id|w.objects.get(id)).filter(|p|p.kind==Kind::Player&&p.alive()&&!p.in_combat()&&p.position.is_some_and(|q|q.distance(&spec.site.arena)<=spec.site.radius)).min_by(|a,b|a.position.unwrap().distance(&pos).total_cmp(&b.position.unwrap().distance(&pos))){if let Some(st)=&mut self.status{st.opponent=Some(opponent.guid);}self.phase("approaching","opponent_selected",w,&mut e);e.events.push(("enemy.encountered".into(),json!({"guid":opponent.guid,"source":"duel_candidate"})));}}},
 "approaching"=>{if let Some(op)=s.opponent.and_then(|id|w.objects.get(&id)){if let Some(p)=op.position{if w.position.is_some_and(|q|q.distance(&p)<=8.){self.phase("challenging","challenge_range",w,&mut e);}else{self.navigate(p,&mut e);}}}else{self.phase("seeking","opponent_lost",w,&mut e);}},
 "challenging"=>{if let Some(id)=s.opponent.filter(|id|spec.opponents.contains(id)){e.actions.extend([Action::Select(id),Action::Cast{spell:7266,target:Some(id)}]);self.phase("awaiting_challenge","challenge_sent",w,&mut e);}},
 "awaiting_challenge"=>{if w.now_ms.saturating_sub(self.phase_at)>30000{self.phase("seeking","challenge_timeout",w,&mut e);}},
 "countdown"=>{if self.arbiter!=0&&self.countdown_until!=0&&w.now_ms>=self.countdown_until{self.phase("duelling","countdown_complete",w,&mut e);self.combat_seen=true;e.actions.push(Action::Stand(0));e.events.push(("combat.started".into(),json!({"mode":"duel"})));}},
 "duelling"=>{if let Some(id)=s.opponent{if let Some(a)=crate::strategy::choose(w,id){e.actions.push(a);}}},
 "recovering"=>{if full(w){e.actions.push(Action::Stand(0));self.phase("checking_supplies","recovered_continue_task",w,&mut e);e.events.push(("task.resumed".into(),json!({"reason":"recovered"})));}else{if me.health_pct().is_some_and(|h|h<99.)&&w.now_ms.saturating_sub(self.last_food)>30000{if let Some(i)=w.inventory.iter().find(|i|FOOD.contains(&i.entry)||i.entry==spec.site.food_entry){e.actions.push(Action::UseItem{bag:i.bag,slot:i.slot});self.last_food=w.now_ms;}}if w.class==8&&me.mana_pct().is_some_and(|m|m<99.)&&w.now_ms.saturating_sub(self.last_water)>30000{if let Some(i)=w.inventory.iter().find(|i|WATER.contains(&i.entry)){e.actions.push(Action::UseItem{bag:i.bag,slot:i.slot});self.last_water=w.now_ms;}}e.actions.push(Action::Stand(1));}},
 _=>{}
 }e
 }
}
