use crate::model::{Action,WorldView};

/// Shared, intentionally conservative first combat policy. It only proposes actions for
/// authoritative targets already present in the protocol view; execution stays in protocol.rs.
pub fn choose(view:&WorldView,target:u64)->Option<Action>{
    let enemy=view.objects.get(&target)?;
    if !enemy.alive() || !enemy.position.is_some() { return None; }
    match view.class {
        8 => { // Mage: keep casting only known, ready spells; movement remains executor-owned.
            for spell in [116u32, 133u32, 118u32, 122u32] { if view.ready(spell) { return Some(Action::Cast{spell,target:Some(target)}); } }
        }
        1 => { // Warrior: use conservative strike actions; no invented rage or target state.
            for spell in [7386u32, 11597u32, 11578u32, 20252u32] { if view.ready(spell) { return Some(Action::Cast{spell,target:Some(target)}); } }
            return Some(Action::Attack(target));
        }
        _ => {}
    }
    None
}
