//! Basic native-request fallback. Chains are ascending 1.12 ranks, not numeric ID order.
//! These are scheduling hints only: authoritative SpellStart/Go/CastResult override them.
use crate::model::{Action, WorldView};
pub const FROSTBOLT: &[u32] = &[116,205,837,7322,8406,8407,8408,10179,10180,10181,25304];
pub const FIREBALL: &[u32] = &[133,143,145,3140,8400,8401,8402,10148,10149,10150,10151,25306];
pub const FIRE_BLAST: &[u32] = &[2136,2137,2138,8412,8413,10197,10199];
pub const FROST_NOVA: &[u32] = &[122,865,6131,10230];
pub const HAMSTRING: &[u32] = &[1715,7372,7373];
pub const HEROIC_STRIKE: &[u32] = &[78,284,285,1608,11564,11565,11566,11567,25286];
pub const CHARGE: &[u32] = &[100,6178,11578];
pub const INTERCEPT: &[u32] = &[20252,20616,20617];
pub fn highest_known(view:&WorldView,chain:&[u32])->Option<u32>{chain.iter().rev().find(|s|view.spells.contains(s)).copied()}
fn ready_rank(view:&WorldView,chain:&[u32])->Option<u32>{highest_known(view,chain).filter(|s|view.ready(*s))}
fn cast(spell:u32,target:Option<u64>)->Action{Action::Cast{spell,target}}
pub fn choose(view:&WorldView,target:u64)->Option<Action>{choose_actions(view,target).into_iter().next()}
pub fn choose_actions(view:&WorldView,target:u64)->Vec<Action>{
 let Some(enemy)=view.objects.get(&target) else{return vec![]};
 if !enemy.alive(){return vec![]}
 let (Some(from),Some(to))=(view.position,enemy.position)else{return vec![]};
 if from.map!=to.map||!from.finite()||!to.finite(){return vec![]}
 let distance=from.distance(&to);
 if view.now_ms<view.casting_until{return vec![]}
 match view.class{
  8=>{
   if distance>30.0{
    // A desired standoff point only. Navigation must project/validate it, never send it directly.
    let dx=from.x-to.x;let dy=from.y-to.y;let horizontal=dx.hypot(dy);
    if horizontal<0.01{return vec![]}
    let mut goal=to;goal.x+=24.0*dx/horizontal;goal.y+=24.0*dy/horizontal;
    return vec![Action::Navigate(goal)]
   }
   if distance<=8.0&&!enemy.auras.iter().any(|a|FROST_NOVA.contains(a)){
    if let Some(s)=ready_rank(view,FROST_NOVA){return vec![cast(s,None)]}
   }
   if enemy.health_pct().is_some_and(|h|h<=20.0){if let Some(s)=ready_rank(view,FIRE_BLAST){return vec![cast(s,Some(target))]}}
   if let Some(s)=ready_rank(view,FROSTBOLT).or_else(||ready_rank(view,FIREBALL)){return vec![cast(s,Some(target))]}
   vec![]
  },
  1=>{
   let Some(me)=view.me()else{return vec![]};
   if (8.0..=25.0).contains(&distance){
    if !me.in_combat(){if me.auras.contains(&2457){if let Some(s)=ready_rank(view,CHARGE){return vec![cast(s,Some(target))]}}else if view.ready(2457){return vec![cast(2457,None)]}}
    else if me.rage.is_some_and(|r|r>=10){if me.auras.contains(&2458){if let Some(s)=ready_rank(view,INTERCEPT){return vec![cast(s,Some(target))]}}else if view.ready(2458){return vec![cast(2458,None)]}}
   }
   if distance>3.0{return vec![Action::Navigate(to)]}
   let mut actions=vec![Action::Attack(target)];
   if me.rage.is_some_and(|r|r>=10)&&!enemy.auras.iter().any(|a|HAMSTRING.contains(a)){
    if let Some(s)=ready_rank(view,HAMSTRING){actions.push(cast(s,Some(target)));return actions}
   }
   if me.rage.is_some_and(|r|r>=30){if let Some(s)=ready_rank(view,HEROIC_STRIKE){actions.push(cast(s,Some(target)))}}
   actions
  },_=>vec![]
 }
}
/// (cast time, cooldown, GCD) in milliseconds. Conservative hints, not DBC validation.
/// Unknown spells get the ordinary GCD and no invented long cooldown.
pub fn spell_timing(spell:u32)->(u64,u64,u64){
 if let Some(rank)=FROSTBOLT.iter().position(|s|*s==spell){return ([1500,1800,2200,2600].get(rank).copied().unwrap_or(3000),0,1500)}
 if let Some(rank)=FIREBALL.iter().position(|s|*s==spell){return ([1500,2000,2500,3000].get(rank).copied().unwrap_or(3500),0,1500)}
 if FIRE_BLAST.contains(&spell){return(0,8000,1500)}
 if FROST_NOVA.contains(&spell){return(0,25000,1500)}
 if CHARGE.contains(&spell){return(0,15000,1500)}
 if INTERCEPT.contains(&spell){return(0,30000,1500)}
 if HAMSTRING.contains(&spell){return(0,0,1500)}
 if HEROIC_STRIKE.contains(&spell){return(0,0,0)}
 if [5504,5505,5506,6127,10138,10139,10140,587,597,990,6129,10144,10145].contains(&spell){return(3000,0,1500)}
 if [3561,3562,3563,3565,3566,3567].contains(&spell){return(10000,0,1500)}
 if [2457,2458,71].contains(&spell){return(0,1000,1000)}
 if spell==7266{return(0,0,0)}
 (0,0,1500)
}
