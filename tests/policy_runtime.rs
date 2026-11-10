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
        class: Some(class),form:Some(0),
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
    { let mut doc:serde_json::Value=serde_json::from_str(include_str!("../config/policies/45-mage-common.json")).unwrap();
 for file in ["50-mage-vs-warrior.json","51-mage-vs-rogue.json","52-mage-vs-warlock.json","53-mage-vs-priest.json","54-mage-vs-hunter.json","55-mage-vs-shaman.json","56-mage-vs-druid.json","57-mage-vs-paladin.json","58-mage-vs-mage.json"] {
 let d:serde_json::Value=serde_json::from_str(&std::fs::read_to_string(format!("{}/config/policies/{file}",env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
 for(key,val)in d["predicates"].as_object().unwrap(){doc["predicates"][key]=val.clone();}
 doc["phases"][0]["rules"].as_array_mut().unwrap().extend(d["phases"][0]["rules"].as_array().unwrap().clone());
 }doc}
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

fn matrix(world:&WorldView)->(Option<Vec<Action>>,String){let p=path();std::fs::write(&p,json().to_string()).unwrap();let mut r=PolicyRuntime::new(Some(p.clone())).unwrap();let a=r.choose(world,2);let status=r.status();std::fs::remove_file(p).unwrap();(a,status)}
#[test]fn matrix_interrupts_known_caster_and_not_unknown_class(){let mut w=world();w.spells.insert(2139);w.objects.get_mut(&2).unwrap().class=Some(9);w.casts.insert(2,CastView{spell:5782,ends_ms:10000});let(a,s)=matrix(&w);assert!(matches!(a.unwrap()[0],Action::Cast{spell:2139,..}));assert!(s.contains("warlock-interrupt-observed-fear"));w.objects.get_mut(&2).unwrap().class=None;assert!(matrix(&w).0.is_none());}
#[test]fn matrix_melee_nova_root_then_open_distance(){let mut w=world();w.spells.insert(10230);w.objects.get_mut(&2).unwrap().position=Some(location(5.));let(a,s)=matrix(&w);assert!(matches!(a.unwrap()[0],Action::Cast{spell:10230,target:None}));assert!(s.contains("warrior-nova-close"));w.objects.get_mut(&2).unwrap().auras.insert(10230);let(a,s)=matrix(&w);assert!(matches!(a.unwrap()[0],Action::Navigate(_)));assert!(s.contains("open-rooted-distance"));}
#[test]fn matrix_hunter_and_paladin_have_distinct_responses(){let mut w=world();w.objects.get_mut(&2).unwrap().class=Some(3);w.objects.get_mut(&2).unwrap().position=Some(location(5.));w.spells.insert(10230);assert!(matrix(&w).1.contains("hunter-nova-close"));w.objects.get_mut(&2).unwrap().class=Some(2);w.spells.insert(2139);w.casts.insert(2,CastView{spell:635,ends_ms:9999});assert!(matrix(&w).1.contains("paladin-interrupt-observed-cast"));}
#[test]fn matrix_low_health_block_and_known_barrier_rank(){let mut w=world();w.spells.extend([11958,13033]);w.objects.get_mut(&1).unwrap().health=Some(20);let(a,s)=matrix(&w);assert!(matches!(a.unwrap()[0],Action::Cast{spell:11958,target:None}));assert!(s.contains("ice-block-emergency"));w.objects.get_mut(&1).unwrap().health=Some(100);let(a,s)=matrix(&w);assert!(matches!(a.unwrap()[0],Action::Cast{spell:13033,target:None}));assert!(s.contains("ice-barrier"));w.objects.get_mut(&1).unwrap().auras.insert(13033);assert!(!matrix(&w).1.contains("ice-barrier"));}
#[test]fn matrix_engaged_multiple_differs_and_priest_frostbolt_remains_legal(){let mut w=world();w.spells.insert(10230);w.objects.get_mut(&2).unwrap().position=Some(location(5.));let mut third=entity(3,4,4.);third.flags=0x80000;third.target=Some(1);w.objects.insert(3,third);assert!(matrix(&w).1.contains("nova-multiple-engaged"));w.objects.remove(&3);w.objects.get_mut(&2).unwrap().class=Some(5);w.objects.get_mut(&2).unwrap().position=Some(location(20.));assert!(matrix(&w).1.contains("priest-frostbolt"));}
#[test]fn matrix_all_native_classes_have_explicit_matchup_guards(){let mut w=world();for class in [1,2,3,4,5,7,8,9,11]{w.objects.get_mut(&2).unwrap().class=Some(class);let(a,s)=matrix(&w);assert!(matches!(a.unwrap()[0],Action::Cast{spell:10181,..}),"{class}: {s}");assert!(s.contains("-frostbolt"));}}
