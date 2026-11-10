//! Client observation adapter and file snapshot lifecycle; no raw packets, GM calls or hidden state.
use crate::{
    model::{Action, Kind, WorldView},
    policy::{self, ActionKind, Observation, Policy},
    strategy,
};
use anyhow::{Result, bail};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};
pub struct PolicyRuntime {
    path: Option<PathBuf>,
    active: Vec<Arc<Policy>>,
    pinned: Vec<Arc<Policy>>,
    files:Vec<String>, pinned_files:Vec<String>,
    signature: String,
    next_poll: u64,
    generation: u64,
    in_fight: bool,
    last_target: u64,
    error: String,
    status: String,
    last_selection:String,
}
impl PolicyRuntime {
    pub fn new(path: Option<PathBuf>) -> Result<Self> {
        let mut p = Self {
            path,
            active: vec![],
            pinned: vec![],
            files:vec![],pinned_files:vec![],
            signature: String::new(),
            next_poll: 0,
            generation: 0,
            in_fight: false,
            last_target: 0,
            error: String::new(),
            status: "fallback:no_policy".into(),
            last_selection:String::new(),
        };
        if p.path.is_some() {
            p.load()?;
        }
        Ok(p)
    }
    pub fn status(&self) -> String {
        if self.error.is_empty() {
            self.status.clone()
        } else {
            format!("{} reload_error={}", self.status, self.error)
        }
    }
    pub fn combat_ended(&mut self) {
        self.in_fight = false;
        self.pinned.clear();self.pinned_files.clear();
    }
    fn load(&mut self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut paths = if path.is_dir() {
            std::fs::read_dir(path)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?
                .into_iter()
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect::<Vec<_>>()
        } else {
            vec![path.clone()]
        };
        paths.sort();
        if paths.len() > 64 {
            bail!("policy file count exceeds64")
        }
        let mut signature = String::new();
        let mut parsed = Vec::new();
        let mut bytes = 0;let mut filenames=Vec::new();
        for path in paths {
            if std::fs::symlink_metadata(&path)?.file_type().is_symlink(){bail!("policy symlinks are not supported")};
            filenames.push(path.file_name().unwrap_or_default().to_string_lossy().into_owned());
            let size = std::fs::metadata(&path)?.len();
            if size > 1024 * 1024 {
                bail!("policy file exceeds1MiB")
            };
            let text = std::fs::read_to_string(&path)?;
            bytes += text.len();
            if bytes > 4 * 1024 * 1024 {
                bail!("policy bundle exceeds4MiB")
            };
            signature.push_str(&format!("{}:{}:{};", path.display(), text.len(), text));
            parsed.push(policy::parse(&text)?);
        }
        if signature != self.signature {
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("generation overflow"))?;
            for p in &mut parsed {
                p.generation = self.generation
            }
            self.active = parsed.into_iter().map(Arc::new).collect();
            self.signature = signature;self.files=filenames;
        }
        self.status = format!(
            "generation={} pinned={} loaded={}",
            self.generation,
            self.pinned.first().map_or(0, |p| p.generation),
            self.active.len()
        );
        Ok(())
    }
    pub fn choose(&mut self, w: &WorldView, target: u64) -> Option<Vec<Action>> {
        self.path.as_ref()?;
        if w.now_ms >= self.next_poll {
            self.next_poll = w.now_ms.saturating_add(1000);
            if let Err(e) = self.load() {
                self.error = e.to_string();
            } else {
                self.error.clear();
            }
        }
        let me = w.me()?;
        let enemy = w.objects.get(&target)?;
        let to = enemy.position?;
        let from = w.position?;
        if !enemy.alive() || from.map != to.map {
            return None;
        }
        let fighting = me.in_combat() || w.duel.as_ref().is_some_and(|d| d.active);
        if !self.in_fight || !fighting {
            self.pinned = self.active.clone();self.pinned_files=self.files.clone();
        }
        self.in_fight = fighting;
        self.last_target = target;
        let mode = if enemy.kind == Kind::Player {
            "world_pvp"
        } else if enemy.kind == Kind::Creature {
            "world_pve"
        } else {
            return None;
        };
        let mut o = Observation {
            mode: mode.into(),
            phase: "engage".into(),
            opponent_count: 1,
            ..Default::default()
        };
        o.numbers.insert("self.class".into(), w.class.into());
        if let Some(n) = me.level {
            o.numbers.insert("self.level".into(), n.into());
        }
        if let Some(n) = me.health_pct() {
            o.numbers.insert("self.health_pct".into(), n.into());
        }
        if let Some(n) = me.mana_pct() {
            o.numbers.insert("self.mana_pct".into(), n.into());
        }
        if let Some(n) = me.rage {
            o.numbers.insert("self.power".into(), n.into());
        } else if let Some(n) = me.mana {
            o.numbers.insert("self.power".into(), n.into());
        }
        if let Some(n)=enemy.form{o.numbers.insert("target.form".into(),n.into());}
        if let Some(cast)=w.casts.get(&target).filter(|c|c.ends_ms>w.now_ms){o.numbers.insert("target.cast_spell".into(),cast.spell.into());}
        if let Some(history)=w.observed_spells.get(&target){
            o.sets.insert("target.observed_spells".into(),history.iter().filter(|(_,at)|w.now_ms.saturating_sub(**at)<=120000).map(|(id,_)|*id).collect());
            o.sets.insert("target.recent_spells".into(),history.iter().filter(|(_,at)|w.now_ms.saturating_sub(**at)<=5000).map(|(id,_)|*id).collect());
        }
        if let Some(n) = enemy.class {
            o.numbers.insert("target.class".into(), n.into());
        }
        if let Some(n) = enemy.level {
            o.numbers.insert("target.level".into(), n.into());
        }
        if let Some(n) = enemy.health_pct() {
            o.numbers.insert("target.health_pct".into(), n.into());
        }
        o.numbers
            .insert("target.distance".into(), from.distance(&to).into());
        // Only the selected target and currently engaged units targeting this actor; not all nearby players.
        let mut engaged = BTreeSet::from([target]);
        for e in w.objects.values() {
            if e.guid != w.self_guid
                && e.in_combat()
                && e.target == Some(w.self_guid)
                && e.position.is_some_and(|p| p.map == from.map)
            {
                engaged.insert(e.guid);
            }
        }
        o.opponent_count = engaged.len() as u32;
        o.numbers
            .insert("nearby.enemy_count".into(), o.opponent_count.into());
        o.flags.insert("self.combat".into(), me.in_combat());
        o.flags
            .insert("self.casting".into(), w.casting_until > w.now_ms);
        o.flags.insert("target.alive".into(), true);
        o.flags.insert(
            "target.casting".into(),
            w.casts.get(&target).is_some_and(|c| c.ends_ms > w.now_ms),
        );
        o.sets.insert("self.aura".into(), me.auras.clone());
        o.sets.insert("target.aura".into(), enemy.auras.clone());
        let mut ready = BTreeSet::new();
        let mut knows = BTreeSet::new();
        for (root, chain) in chains() {
            if let Some(id) = strategy::highest_known(w, chain) {
                knows.insert(root);
                if strategy::policy_ready(w,id) {
                    ready.insert(root);
                }
            }
        }
        o.sets.insert("self.ready_spells".into(), ready);
        o.sets.insert("self.knows".into(), knows);
        o.targets.insert("self".into(), w.self_guid);
        o.targets.insert("primary_enemy".into(), target);
        // All applicable files contribute candidates. Priority wins globally; filename order
        // only breaks equal scores, so a common fallback cannot mask a matchup interrupt.
        let mut ranked=Vec::new();let mut unknown=false;
        for(index,policy)in self.pinned.iter().enumerate(){
            let mut ctx=o.clone();if policy.doc.scope.mode=="duel"&&w.duel.as_ref().is_some_and(|d|d.active){ctx.mode="duel".into();}
            let(candidates,traces)=policy::rank(policy,&ctx,w.now_ms,64);
            unknown|=traces.iter().any(|t|t.unknown);
            for candidate in candidates{ranked.push((index,candidate));}
        }
        ranked.sort_by(|(ia,a),(ib,b)|b.priority.cmp(&a.priority).then(ia.cmp(ib)).then(a.rule.cmp(&b.rule)));
        for(index,candidate)in ranked{
            if candidate.expires_ms<=w.now_ms{continue;}
            if let Some(actions)=translate(w,target,&candidate.action){
                let file=self.pinned_files.get(index).map_or("<memory>",String::as_str);
                self.last_selection=format!("last_policy={} last_rule={}",file,candidate.rule);
                self.status=format!("generation={} policy={} target_class={} opponents={} rule={} decision=matched",candidate.generation,file,enemy.class.map_or_else(||"unknown".into(),|c|c.to_string()),o.opponent_count,candidate.rule);
                return Some(actions);
            }
        }
        self.status=format!("generation={} decision={} {}",self.generation,if unknown{"unknown_observation"}else{"no_applicable_rule"},self.last_selection);
        None
    }
}
fn chains() -> Vec<(u32, &'static [u32])> {
    vec![
        (116, strategy::FROSTBOLT),
        (133, strategy::FIREBALL),
        (2136, strategy::FIRE_BLAST),
        (122, strategy::FROST_NOVA),
        (2139, strategy::COUNTERSPELL),
        (118,strategy::POLYMORPH),
        (475,strategy::REMOVE_CURSE),
        (120, strategy::CONE_OF_COLD),
        (11426, strategy::ICE_BARRIER),
        (11958, strategy::ICE_BLOCK),
        (1715, strategy::HAMSTRING),
        (78, strategy::HEROIC_STRIKE),
        (100, strategy::CHARGE),
    ]
}
fn translate(w: &WorldView, target: u64, a: &policy::ActionSpec) -> Option<Vec<Action>> {
    if a.kind == ActionKind::Wait {
        return Some(vec![]);
    }
    if a.kind == ActionKind::Stop {
        return Some(vec![Action::StopMoving]);
    }
    if a.kind == ActionKind::Attack {
        return Some(vec![Action::Attack(target)]);
    }
    if a.ability == "combat.approach" {
        let mut p = w.objects.get(&target)?.position?;
        if w.class == 8 {
            let me = w.position?;
            let (dx, dy) = (me.x - p.x, me.y - p.y);
            let n = dx.hypot(dy);
            if n < 0.01 {
                return None;
            };
            p.x += 24. * dx / n;
            p.y += 24. * dy / n;
        }
        return Some(vec![Action::Navigate(p)]);
    }
    let chain = match a.ability.as_str() {
        "mage.frostbolt" => strategy::FROSTBOLT,
        "mage.fireball" => strategy::FIREBALL,
        "mage.fire_blast" => strategy::FIRE_BLAST,
        "mage.frost_nova" => strategy::FROST_NOVA,
        "mage.counterspell" => strategy::COUNTERSPELL,
        "mage.polymorph"=>strategy::POLYMORPH,
        "mage.remove_curse"=>strategy::REMOVE_CURSE,
        "mage.cone_of_cold" => strategy::CONE_OF_COLD,
        "mage.ice_barrier" => strategy::ICE_BARRIER,
        "mage.ice_block" => strategy::ICE_BLOCK,
        "warrior.hamstring" => strategy::HAMSTRING,
        "warrior.heroic_strike" => strategy::HEROIC_STRIKE,
        "warrior.charge" => strategy::CHARGE,
        _ => return None,
    };
    let spell = strategy::highest_known(w, chain).filter(|s| strategy::policy_ready(w,*s))?;
    let mut out = if w.class == 1 {
        vec![Action::Attack(target)]
    } else {
        vec![]
    };
    if w.now_ms<w.casting_until{if let Some(spell)=w.pending_spell{if strategy::COUNTERSPELL.contains(&spell)||strategy::ICE_BLOCK.contains(&spell){return None;}out.push(Action::CancelCast(spell));}}
    out.push(Action::Cast {
        spell,
        target: if a.target == "self" {
            None
        } else {
            Some(target)
        },
    });
    Some(out)
}
