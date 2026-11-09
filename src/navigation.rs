//! Local MMap/Detour navigation, with bounded ground-only paths. No network or teleport API.
use crate::model::Position;
use anyhow::{Result,bail};
use std::{ffi::{CString,CStr},path::{Path,PathBuf},time::Duration};
unsafe extern "C" {fn ra_nav_path(root:*const std::ffi::c_char,map:u32,start:*const f32,end:*const f32,output:*mut f32,capacity:i32,error:*mut std::ffi::c_char,error_capacity:usize)->i32;}
pub struct Navigator {root:PathBuf}
impl Navigator {
 pub fn new(root:&Path)->Self{Self{root:root.to_owned()}}
 pub fn path(&self,from:Position,to:Position)->Result<Vec<Position>>{
  if from.map!=to.map||!from.finite()||!to.finite(){bail!("navigation requires finite same-map positions")}
  let root=CString::new(self.root.to_string_lossy().as_bytes())?;let mut output=vec![0f32;4096*3];let mut error=vec![0i8;512];
  let count=unsafe{ra_nav_path(root.as_ptr(),from.map,[from.x,from.y,from.z].as_ptr(),[to.x,to.y,to.z].as_ptr(),output.as_mut_ptr(),4096,error.as_mut_ptr(),error.len())};
  if count<0{bail!("{}",unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy())}
  Ok(output[..count as usize*3].chunks_exact(3).map(|p|Position{map:from.map,x:p[0],y:p[1],z:p[2],orientation:from.orientation}).collect())
 }
}
pub struct Follower {points:Vec<Position>,next:usize,stopped:bool}
impl Follower {
 pub fn new(points:Vec<Position>)->Result<Self>{if points.len()<2||points.iter().any(|p|!p.finite()||p.map!=points[0].map){bail!("invalid navigation path")};Ok(Self{points,next:1,stopped:false})}
 pub fn stop(&mut self){self.stopped=true;}
 pub fn is_done(&self)->bool{self.stopped||self.next>=self.points.len()}
 pub fn step(&mut self,current:Position,speed:f32,dt:Duration)->Result<Option<Position>>{
  if self.is_done(){return Ok(None)}
  if !current.finite()||current.map!=self.points[0].map||!speed.is_finite()||!(0.0..=20.0).contains(&speed){bail!("invalid movement state")}
  if current.distance(&self.points[self.next-1])>100.0{bail!("position diverged from route")}
  let mut budget=speed*dt.min(Duration::from_millis(200)).as_secs_f32();let mut out=current;
  while self.next<self.points.len(){let target=self.points[self.next];let distance=out.distance(&target);if distance<0.02{self.next+=1;continue}if budget<=0.0{break}let ratio=(budget/distance).min(1.0);out.orientation=(target.y-out.y).atan2(target.x-out.x);out.x+=(target.x-out.x)*ratio;out.y+=(target.y-out.y)*ratio;out.z+=(target.z-out.z)*ratio;budget-=distance*ratio;if ratio>=1.0{self.next+=1}else{break}}
  Ok(Some(out))
 }
}
#[cfg(test)]mod tests{use super::*;
 fn p(x:f32)->Position{Position{x,map:1,..Position::default()}}
 #[test]fn bounded_step_and_stop(){let mut f=Follower::new(vec![p(0.),p(10.)]).unwrap();let n=f.step(p(0.),7.,Duration::from_secs(2)).unwrap().unwrap();assert!((n.x-1.4).abs()<0.001);f.stop();assert!(f.step(n,7.,Duration::from_secs(1)).unwrap().is_none());}
 #[test]fn reject_bad_route(){assert!(Follower::new(vec![p(0.)]).is_err());let mut q=p(1.);q.map=0;assert!(Follower::new(vec![p(0.),q]).is_err());}
}
