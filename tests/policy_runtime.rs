use realm_agent::{model::*, policy, policy_runtime::PolicyRuntime};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn location(x: f32) -> Position {
    Position {
        map: 1,
        x,
        ..Default::default()
    }
}
fn entity(guid: u64, class: u8, x: f32) -> Entity {
    Entity {
        guid,
        kind: Kind::Player,
        entry: 0,
        name: None,
        position: Some(location(x)),
        health: Some(100),
        max_health: Some(100),
        mana: Some(100),
        max_mana: Some(100),
        rage: None,
        level: Some(60),
        class: Some(class),
        flags: 0,duel_arbiter:0,
        auras: BTreeSet::new(),
        target: None,
        updated_ms: 0,
    }
}
fn world() -> WorldView {
    let mut w = WorldView {
        self_guid: 1,
        class: 8,
        position: Some(location(0.)),
        now_ms: 100,
        ..Default::default()
    };
    w.objects.insert(1, entity(1, 8, 0.));
    w.objects.insert(2, entity(2, 1, 20.));
    w.spells.insert(10181);
    w
}
fn path() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "policy-runtime-{}-{}.json",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}
fn json() -> serde_json::Value {
    serde_json::from_str(include_str!("../config/policies/50-city-pvp.json")).unwrap()
}
#[test]
fn real_json_rank_highest_and_hot_reload_pin() {
    let p = path();
    std::fs::write(&p, json().to_string()).unwrap();
    let mut r = PolicyRuntime::new(Some(p.clone())).unwrap();
    let mut w = world();
    assert!(matches!(
        r.choose(&w, 2).unwrap()[0],
        Action::Cast { spell: 10181, .. }
    ));
    w.objects.get_mut(&1).unwrap().flags = 0x80000;
    w.now_ms = 200;
    r.choose(&w, 2);
    std::fs::write(&p, "bad").unwrap();
    w.now_ms = 1200;
    assert!(r.choose(&w, 2).is_some());
    assert!(r.status().contains("generation="));
    let mut j = json();
    j["enabled"] = false.into();
    std::fs::write(&p, j.to_string()).unwrap();
    w.now_ms = 2200;
    assert!(r.choose(&w, 2).is_some());
    w.objects.get_mut(&1).unwrap().flags = 0;
    w.now_ms = 2300;
    assert!(r.choose(&w, 2).is_none());
    std::fs::remove_file(p).unwrap();
}
#[test]
fn parser_rejects_semantic_holes() {
    let j = json();
    for bad in ["{\"schema_version\":1,\"schema_version\":1}", "{\"a\":1}"] {
        assert!(policy::parse(bad).is_err());
    }
    for mutate in 0..5 {
        let mut x = j.clone();
        match mutate {
            0 => {
                x["phases"][0]["rules"][0]["when"] =
                    serde_json::json!({"field":"self.class","op":"eq","not_value":8})
            }
            1 => x["phases"][0]["rules"][0]["ttl_ms"] = 0.into(),
            2 => x["phases"][0]["rules"][0]["action"]["kind"] = "attack".into(),
            3 => x["predicates"] = serde_json::json!({"cycle":{"ref":"cycle"}}),
            _ => {
                x["phases"][0]["rules"][0]["when"] = serde_json::json!({"field":"self.ready_spells","op":"contains","value":4294967296u64})
            }
        };
        assert!(policy::parse(&x.to_string()).is_err());
    }
}
#[test]
fn wrong_mode_and_no_file_fall_back() {
    let mut r = PolicyRuntime::new(None).unwrap();
    assert!(r.choose(&world(), 2).is_none());
    let mut p = policy::parse(&json().to_string()).unwrap();
    p.generation = 1;
    let o = policy::Observation {
        mode: "world_pve".into(),
        phase: "engage".into(),
        opponent_count: 1,
        ..Default::default()
    };
    assert!(policy::rank(&p, &o, 100, 64).0.is_empty());
}
