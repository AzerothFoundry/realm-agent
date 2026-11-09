#include "BotDecision.h"
#include <algorithm>
#include <limits>
#include <utility>

namespace BotDecision {
namespace {
bool Renewal(Intent const& a, Intent const& b) {
    return a.owner == b.owner && a.rule == b.rule && a.action.ability == b.action.ability &&
           a.action.target == b.action.target && a.targetGuid == b.targetGuid && a.resources == b.resources;
}
bool Conflicts(Intent const& a, Intent const& b) {
    return (a.resources & b.resources) || (a.owner == b.owner && a.rule == b.rule);
}
}
bool Scope::operator==(Scope const& other) const {
    return actor == other.actor && actorGeneration == other.actorGeneration &&
           sceneGeneration == other.sceneGeneration && map == other.map && instance == other.instance && group == other.group;
}
FieldCatalog FieldCatalog::Standard() {
    FieldCatalog catalog;
    for (auto field : {"self.class", "self.role", "self.level", "self.health_pct", "self.mana_pct",
                       "target.entry", "target.health_pct", "target.distance", "enemies.engaged",
                       "scene.map", "scene.instance", "encounter.elapsed_ms", "phase.elapsed_ms"})
        catalog.fields[field] = ValueType::Number;
    for (auto field : {"self.alive", "self.combat", "self.casting", "self.moving", "self.controlled",
                       "target.present", "target.alive", "target.casting", "target.interruptible", "aoe.safe"})
        catalog.fields[field] = ValueType::Flag;
    for (auto field : {"scene.kind", "encounter.id", "encounter.phase", "assignment.role"})
        catalog.fields[field] = ValueType::Text;
    for (auto field : {"self.aura", "target.aura", "target.own_aura", "self.knows", "self.ready_spells"})
        catalog.fields[field] = ValueType::IdSet;
    return catalog;
}
CapabilityCatalog FoundationCapabilities() {
    CapabilityCatalog c;
    c.abilities["warrior.sunder_armor"] = {ActionKind::Cast, 7386, Casting, {"enemy"}};
    c.abilities["mage.frostbolt"] = {ActionKind::Cast, 116, Casting | Movement, {"enemy"}};
    c.abilities["priest.heal"] = {ActionKind::Cast, 2054, Casting | Movement, {"self"}};
    return c;
}
EventBuffer::EventBuffer(size_t capacity) : m_capacity(std::min<size_t>(capacity, 4096)) {}
void EventBuffer::Push(DecisionEvent event) {
    // A caller cannot accidentally turn untrusted diagnostic strings into an unbounded log.
    for (auto text : {&event.owner, &event.rule, &event.stage, &event.reason, &event.encounter})
        if (text->size() > 256) text->resize(256);
    if (!m_capacity) { ++m_dropped; return; }
    if (m_events.size() == m_capacity) { m_events.pop_front(); ++m_dropped; }
    m_events.push_back(std::move(event));
}
std::vector<DecisionEvent> EventBuffer::Drain() {
    std::vector<DecisionEvent> output(m_events.begin(), m_events.end());
    m_events.clear();
    return output;
}
Result Validate(Intent const& intent, Observation const& current) {
    if (!current.scope.actor || !current.scope.actorGeneration || !current.scope.sceneGeneration ||
        intent.scope != current.scope)
        return {false, "stale_scope"};
    if (!intent.observation || intent.observation > current.sequence || !intent.policyGeneration)
        return {false, "invalid_version"};
    if (intent.encounter != current.encounter || intent.encounterAttempt != current.encounterAttempt ||
        intent.encounterEpoch != current.encounterEpoch ||
        (!current.encounter.empty() && (!current.encounterAttempt || !current.encounterEpoch)))
        return {false, "stale_encounter"};
    if (intent.issuedMs > current.nowMs || intent.expiresMs <= current.nowMs ||
        intent.expiresMs <= intent.issuedMs || intent.expiresMs - intent.issuedMs > 30000)
        return {false, "expired_or_invalid_deadline"};
    if (intent.owner.empty() || intent.owner.size() > 128 || intent.rule.empty() || intent.rule.size() > 128 ||
        intent.action.ability.empty() || intent.action.ability.size() > 128 || intent.action.target.size() > 128 ||
        (intent.resources & ~(Movement | Casting | Interaction)))
        return {false, "invalid_intent"};
    if (!intent.action.target.empty()) {
        auto target = current.targets.find(intent.action.target);
        if (target == current.targets.end() || !target->second || intent.targetGuid != target->second)
            return {false, "stale_target"};
    } else if (intent.targetGuid) return {false, "invalid_target"};
    return {true, "valid"};
}
void Arbiter::Reset(Scope const& scope) {
    m_scope = scope;
    m_initialized = true;
    m_encounter.clear(); m_attempt = 0; m_epoch = 0;
    m_leases.clear();
}
std::vector<Intent> Arbiter::Active(uint64_t nowMs) {
    m_leases.erase(std::remove_if(m_leases.begin(), m_leases.end(), [nowMs](Intent const& lease) {
        return lease.expiresMs <= nowMs;
    }), m_leases.end());
    return m_leases;
}
Result Arbiter::Acquire(Intent const& intent, Observation const& current) {
    auto valid = Validate(intent, current);
    if (!valid.ok) return valid;
    if (!m_initialized || m_scope != current.scope || m_encounter != current.encounter ||
        m_attempt != current.encounterAttempt || m_epoch != current.encounterEpoch) {
        Reset(current.scope); m_encounter = current.encounter;
        m_attempt = current.encounterAttempt; m_epoch = current.encounterEpoch;
    }
    Active(current.nowMs);
    for (auto const& lease : m_leases) {
        if (!Conflicts(lease, intent)) continue;
        if (Renewal(lease, intent)) continue;
        if (!lease.interruptible || intent.priority <= lease.priority)
            return {false, "resource_owned_by:" + lease.owner};
    }
    // Only mutate ownership after every conflicting lease has accepted preemption.
    m_leases.erase(std::remove_if(m_leases.begin(), m_leases.end(), [&](Intent const& lease) {
        return Conflicts(lease, intent);
    }), m_leases.end());
    if (intent.resources != None) m_leases.push_back(intent);
    return {true, "acquired"};
}
void Arbiter::Release(std::string const& owner) {
    m_leases.erase(std::remove_if(m_leases.begin(), m_leases.end(), [&](Intent const& lease) {
        return lease.owner == owner;
    }), m_leases.end());
}
Result Dispatch(std::vector<Intent> const& candidates, Observation const& current, Arbiter& arbiter,
                Executor& executor, EventBuffer& events, bool shadow, size_t maxAttempts) {
    auto record = [&](Intent const& intent, char const* stage, Result const& result) {
        DecisionEvent event;
        event.scope = intent.scope; event.observation = intent.observation;
        event.policyGeneration = intent.policyGeneration; event.timeMs = current.nowMs;
        event.owner = intent.owner; event.rule = intent.rule; event.stage = stage; event.reason = result.reason;
        event.encounter = intent.encounter; event.encounterAttempt = intent.encounterAttempt;
        event.encounterEpoch = intent.encounterEpoch;
        events.Push(std::move(event));
    };
    maxAttempts = std::min<size_t>(maxAttempts, 64);
    Result last{false, "no_candidate"};
    for (size_t i = 0; i < candidates.size() && i < maxAttempts; ++i) {
        auto const& intent = candidates[i];
        last = Validate(intent, current);
        if (!last.ok) { record(intent, "validate", last); continue; }
        last = executor.Check(intent, current);
        if (!last.ok) { record(intent, "native_check", last); continue; }
        // A shadow observation never changes actor action ownership or game state.
        if (shadow) { last = {true, "shadow_eligible"}; record(intent, "shadow", last); return last; }
        Arbiter before = arbiter;
        std::vector<Intent> displaced;
        for (auto const& lease : before.Active(current.nowMs))
            if (lease.scope == intent.scope && lease.encounter == intent.encounter &&
                lease.encounterAttempt == intent.encounterAttempt && lease.encounterEpoch == intent.encounterEpoch &&
                Conflicts(lease, intent) && !Renewal(lease, intent))
                displaced.push_back(lease);
        last = arbiter.Acquire(intent, current);
        if (!last.ok) { record(intent, "arbitrate", last); continue; }
        last = executor.Execute(intent, current, displaced);
        record(intent, "execute", last);
        if (last.ok) {
            for (auto const& old : displaced) record(old, "preempted", {true, "replaced_by:" + intent.owner});
            return last;
        }
        // Execute rejection means no action committed; retain previous owners.
        arbiter = std::move(before);
    }
    return last;
}
}
