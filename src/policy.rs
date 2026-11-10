use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Cast,
    Attack,
    Move,
    Stop,
    UseItem,
    AcceptDuel,
    CancelDuel,
    Wait,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSpec {
    pub kind: ActionKind,
    pub ability: String,
    pub target: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub mode: String,
    #[serde(default)]
    pub min_opponents: u32,
    #[serde(default)]
    pub max_opponents: u32,
}
#[derive(Clone, Debug)]
pub enum Truth {
    False,
    True,
    Unknown,
}
#[derive(Clone, Debug)]
pub enum Expr {
    Leaf {
        field: String,
        op: String,
        value: Value,
    },
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Not(Box<Expr>),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDoc {
    pub id: String,
    pub owner: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub priority: i32,
    #[serde(default = "default_ttl")]
    pub ttl_ms: u64,
    pub when: Value,
    pub action: ActionSpec,
}
fn default_ttl() -> u64 {
    500
}
fn default_enabled() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseDoc {
    pub id: String,
    #[serde(default)]
    pub min_hold_ms: u64,
    pub rules: Vec<RuleDoc>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDoc {
    pub schema_version: u32,
    pub revision: u64,
    pub enabled: bool,
    pub scope: Scope,
    #[serde(default)]
    pub predicates: BTreeMap<String, Value>,
    pub phases: Vec<PhaseDoc>,
}
#[derive(Clone, Debug)]
pub struct Rule {
    pub doc: RuleDoc,
    pub expr: Expr,
}
#[derive(Clone, Debug)]
pub struct Policy {
    pub doc: PolicyDoc,
    pub generation: u64,
    pub phases: BTreeMap<String, Vec<Rule>>,
}
#[derive(Clone, Debug, Default)]
pub struct Observation {
    pub mode: String,
    pub phase: String,
    pub opponent_count: u32,
    pub numbers: BTreeMap<String, f64>,
    pub flags: BTreeMap<String, bool>,
    pub sets: BTreeMap<String, BTreeSet<u32>>,
    pub texts: BTreeMap<String, String>,
    pub targets: BTreeMap<String, u64>,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub rule: String,
    pub owner: String,
    pub action: ActionSpec,
    pub priority: i32,
    pub expires_ms: u64,
    pub generation: u64,
}
#[derive(Clone, Debug)]
pub struct Trace {
    pub rule: String,
    pub eligible: bool,
    pub unknown: bool,
    pub reason: String,
}
fn field_type(s: &str) -> Option<&'static str> {
    match s {
        "self.class" | "self.level" | "self.health_pct" | "self.mana_pct" | "self.power"
        | "target.class" | "target.level" | "target.health_pct" | "target.distance"
        | "nearby.enemy_count" => Some("number"),
        "self.combat" | "self.casting" | "self.controlled" | "target.casting" | "target.alive" => {
            Some("flag")
        }
        "self.ready_spells" | "self.knows" | "self.aura" | "target.aura" => Some("set"),
        "scene.kind" => Some("text"),
        _ => None,
    }
}
fn capability(a: &ActionSpec) -> bool {
    match a.ability.as_str() {
        "mage.frostbolt"
        | "mage.fireball"
        | "mage.fire_blast"
        | "warrior.hamstring"
        | "warrior.charge"
        | "warrior.heroic_strike" => a.kind == ActionKind::Cast && a.target == "primary_enemy",
        "mage.frost_nova" => a.kind == ActionKind::Cast && a.target == "self",
        "combat.attack" => a.kind == ActionKind::Attack && a.target == "primary_enemy",
        "combat.approach" => a.kind == ActionKind::Move && a.target == "primary_enemy",
        "travel.stop" => a.kind == ActionKind::Stop && a.target == "none",
        "decision.wait" => a.kind == ActionKind::Wait && a.target == "none",
        _ => false,
    }
}
fn ident(s: &str) -> Result<()> {
    if s.is_empty() || s.len() > 128 || s.chars().any(char::is_control) {
        bail!("invalid identifier")
    };
    Ok(())
}
fn expr(
    v: &Value,
    preds: &BTreeMap<String, Value>,
    d: u32,
    n: &mut u32,
    total: &mut u32,
    stack: &mut BTreeSet<String>,
) -> Result<Expr> {
    *n += 1;
    *total += 1;
    if d > 8 || *n > 256 || *total > 16384 {
        bail!("expression budget exceeded")
    };
    let o = v
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("expression object"))?;
    if let Some(f) = o.get("field") {
        let f = f.as_str().ok_or_else(|| anyhow::anyhow!("field"))?;
        let ty = field_type(f).ok_or_else(|| anyhow::anyhow!("unregistered field"))?;
        let op = o
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("operator"))?;
        let value = o
            .get("value")
            .ok_or_else(|| anyhow::anyhow!("missing condition value"))?;
        if o.len() != 3 {
            bail!("invalid condition keys")
        }
        let valid = match ty {
            "number" => {
                ["eq", "ne", "lt", "lte", "gt", "gte"].contains(&op)
                    && value
                        .as_f64()
                        .is_some_and(|n| n.is_finite() && n.abs() <= 1e12)
            }
            "flag" => ["eq", "ne"].contains(&op) && value.is_boolean(),
            "text" => ["eq", "ne"].contains(&op) && value.as_str().is_some_and(|s| s.len() <= 128),
            "set" => {
                ["contains", "not_contains"].contains(&op)
                    && value.as_u64().is_some_and(|n| n <= u32::MAX as u64)
            }
            _ => false,
        };
        if !valid {
            bail!("condition type or operator mismatch")
        };
        return Ok(Expr::Leaf {
            field: f.into(),
            op: op.into(),
            value: value.clone(),
        });
    }
    if o.len() != 1 {
        bail!("one expression operator required")
    }
    if let Some(x) = o.get("not") {
        return Ok(Expr::Not(Box::new(expr(x, preds, d + 1, n, total, stack)?)));
    }
    for k in ["all", "any"] {
        if let Some(x) = o.get(k) {
            let a = x
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("expression list"))?;
            if a.is_empty() || a.len() > 32 {
                bail!("expression list size")
            };
            let mut r = Vec::new();
            for y in a {
                r.push(expr(y, preds, d + 1, n, total, stack)?)
            }
            return Ok(if k == "all" {
                Expr::All(r)
            } else {
                Expr::Any(r)
            });
        }
    }
    if let Some(r) = o.get("ref") {
        let name = r.as_str().ok_or_else(|| anyhow::anyhow!("predicate ref"))?;
        if !stack.insert(name.into()) {
            bail!("predicate reference cycle")
        };
        let result = expr(
            preds
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("missing predicate"))?,
            preds,
            d + 1,
            n,
            total,
            stack,
        );
        stack.remove(name);
        return result;
    }
    bail!("unknown expression")
}
fn truth(e: &Expr, o: &Observation) -> Truth {
    match e {
        Expr::Leaf { field, op, value } => {
            if let Some(x) = o.numbers.get(field) {
                if !x.is_finite() { return Truth::Unknown; }
                if let Some(y) = value.as_f64() {
                    return cmp(*x, y, op);
                }
            }
            if let Some(x) = o.flags.get(field) {
                if let Some(y) = value.as_bool() {
                    return if (op == "eq" && *x == y) || (op == "ne" && *x != y) {
                        Truth::True
                    } else {
                        Truth::False
                    };
                }
            }
            if let Some(x) = o.texts.get(field) {
                if let Some(y) = value.as_str() {
                    return if (op == "eq" && x == y) || (op == "ne" && x != y) {
                        Truth::True
                    } else {
                        Truth::False
                    };
                }
            }
            if let Some(x) = o.sets.get(field) {
                if let Some(y) = value.as_u64() {
                    let b = x.contains(&(y as u32));
                    return if op == "contains" {
                        if b { Truth::True } else { Truth::False }
                    } else if op == "not_contains" {
                        if b { Truth::False } else { Truth::True }
                    } else {
                        Truth::Unknown
                    };
                }
            }
            Truth::Unknown
        }
        Expr::Not(x) => match truth(x, o) {
            Truth::True => Truth::False,
            Truth::False => Truth::True,
            Truth::Unknown => Truth::Unknown,
        },
        Expr::All(xs) => fold(xs, o, true),
        Expr::Any(xs) => fold(xs, o, false),
    }
}
fn cmp(x: f64, y: f64, op: &str) -> Truth {
    let b = match op {
        "eq" => x == y,
        "ne" => x != y,
        "lt" => x < y,
        "lte" => x <= y,
        "gt" => x > y,
        "gte" => x >= y,
        _ => return Truth::Unknown,
    };
    if b { Truth::True } else { Truth::False }
}
fn fold(xs: &[Expr], o: &Observation, all: bool) -> Truth {
    let mut u = false;
    for x in xs {
        match truth(x, o) {
            Truth::False if all => return Truth::False,
            Truth::True if !all => return Truth::True,
            Truth::Unknown => u = true,
            _ => {}
        }
    }
    if u {
        Truth::Unknown
    } else if all {
        Truth::True
    } else {
        Truth::False
    }
}
pub fn parse(text: &str) -> Result<Policy> {
    if text.len() > 1024 * 1024 {
        bail!("policy too large")
    };
    let strict: StrictValue = serde_json::from_str(text)?;
    let doc: PolicyDoc = serde_json::from_value(strict.0)?;
    if doc.schema_version != 1
        || doc.phases.is_empty()
        || doc.phases.len() > 16
        || doc.predicates.len() > 64
    {
        bail!("unsupported policy or phase limit")
    }
    if !["world_pve", "world_pvp", "duel"].contains(&doc.scope.mode.as_str())
        || doc.scope.max_opponents > 64
        || doc.scope.min_opponents > doc.scope.max_opponents && doc.scope.max_opponents != 0
    {
        bail!("invalid scope")
    }
    let mut total = 0;
    let mut phases = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut count = 0;
    for (name, value) in &doc.predicates {
        ident(name)?;
        expr(
            value,
            &doc.predicates,
            0,
            &mut 0,
            &mut total,
            &mut BTreeSet::new(),
        )?;
    }
    for p in &doc.phases {
        ident(&p.id)?;
        if phases.contains_key(&p.id) || p.min_hold_ms > 86400000 {
            bail!("duplicate phase or invalid hold")
        };
        let mut rules = Vec::new();
        for r in &p.rules {
            count += 1;
            if count > 512 {
                bail!("rule budget exceeded")
            };
            ident(&r.id)?;
            ident(&r.owner)?;
            if !ids.insert(r.id.clone())
                || !(-100000..=100000).contains(&r.priority)
                || r.ttl_ms == 0
                || r.ttl_ms > 5000
            {
                bail!("duplicate rule or invalid priority/TTL")
            }
            if !capability(&r.action) {
                bail!("unsupported ability kind or target")
            };
            rules.push(Rule {
                doc: r.clone(),
                expr: expr(
                    &r.when,
                    &doc.predicates,
                    0,
                    &mut 0,
                    &mut total,
                    &mut BTreeSet::new(),
                )?,
            });
        }
        phases.insert(p.id.clone(), rules);
    }
    Ok(Policy {
        doc,
        generation: 0,
        phases,
    })
}
pub fn rank(p: &Policy, o: &Observation, now: u64, max: usize) -> (Vec<Candidate>, Vec<Trace>) {
    let mut c = Vec::new();
    let mut t = Vec::new();
    if !p.doc.enabled || p.doc.scope.mode != o.mode {
        return (c, t);
    }
    for r in p.phases.get(&o.phase).into_iter().flatten() {
        if now.checked_add(r.doc.ttl_ms).is_none() {
            continue;
        }
        let m = truth(&r.expr, o);
        let tr = match m {
            Truth::True => Trace {
                rule: r.doc.id.clone(),
                eligible: true,
                unknown: false,
                reason: "matched".into(),
            },
            Truth::False => Trace {
                rule: r.doc.id.clone(),
                eligible: false,
                unknown: false,
                reason: "condition_false".into(),
            },
            Truth::Unknown => Trace {
                rule: r.doc.id.clone(),
                eligible: false,
                unknown: true,
                reason: "unknown_observation".into(),
            },
        };
        t.push(tr);
        if matches!(m, Truth::True)
            && r.doc.enabled
            && o.opponent_count >= p.doc.scope.min_opponents
            && (p.doc.scope.max_opponents == 0 || o.opponent_count <= p.doc.scope.max_opponents)
        {
            c.push(Candidate {
                rule: r.doc.id.clone(),
                owner: r.doc.owner.clone(),
                action: r.doc.action.clone(),
                priority: r.doc.priority,
                expires_ms: now.saturating_add(r.doc.ttl_ms),
                generation: p.generation,
            })
        }
    }
    c.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.rule.cmp(&b.rule)));
    c.truncate(if max == 0 { 64 } else { max.min(64) });
    (c, t)
}
pub struct Store {
    active: Arc<Mutex<Arc<Policy>>>,
    pending: Mutex<Option<Policy>>,
    generation: Mutex<u64>,
}
impl Store {
    pub fn new(p: Policy) -> Self {
        Self {
            active: Arc::new(Mutex::new(Arc::new(p))),
            pending: Mutex::new(None),
            generation: Mutex::new(0),
        }
    }
    pub fn stage(&self, text: &str) -> Result<()> {
        *self.pending.lock().unwrap() = Some(parse(text)?);
        Ok(())
    }
    pub fn publish(&self) -> Result<Arc<Policy>> {
        let mut p = self
            .pending
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow::anyhow!("no pending"))?;
        let mut g = self.generation.lock().unwrap();
        *g += 1;
        p.generation = *g;
        let a = Arc::new(p);
        *self.active.lock().unwrap() = a.clone();
        Ok(a)
    }
    pub fn reload_file(&self, path: &Path) -> Result<Arc<Policy>> {
        let text = std::fs::read_to_string(path)?;
        self.stage(&text)?;
        self.publish()
    }
    pub fn current(&self) -> Arc<Policy> {
        self.active.lock().unwrap().clone()
    }
}

// serde_json::Value normally overwrites duplicate object keys; reject before typed decoding.
struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "JSON without duplicate keys")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<StrictValue>()? {
                    v.push(x.0)
                }
                Ok(StrictValue(Value::Array(v)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut m = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if m.contains_key(&k) {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                    let v = a.next_value::<StrictValue>()?;
                    m.insert(k, v.0);
                }
                Ok(StrictValue(Value::Object(m)))
            }
        }
        d.deserialize_any(V)
    }
}
