// GPL-2.0-or-later. Value-only contracts: no Player/Unit pointers cross decision boundaries.
#ifndef MANGOS_BOT_DECISION_H
#define MANGOS_BOT_DECISION_H
#include <cstdint>
#include <deque>
#include <map>
#include <set>
#include <string>
#include <vector>

namespace BotDecision {
struct Result {
    bool ok = false;
    std::string reason;
};
struct Scope {
    uint64_t actor = 0, actorGeneration = 0, sceneGeneration = 0;
    uint32_t map = 0, instance = 0;
    uint64_t group = 0;
    bool operator==(Scope const& other) const;
    bool operator!=(Scope const& other) const { return !(*this == other); }
};
enum class ValueType { Number, Flag, Text, IdSet };
// Explicit field catalog; adding a field requires a producer, semantics and tests.
struct FieldCatalog {
    std::map<std::string, ValueType> fields;
    uint32_t version = 1;
    static FieldCatalog Standard();
};
struct Observation {
    Scope scope;
    uint64_t sequence = 0, nowMs = 0;
    std::string encounter;
    uint64_t encounterAttempt = 0, encounterEpoch = 0;
    std::map<std::string, double> numbers;
    std::map<std::string, bool> flags;
    std::map<std::string, std::string> texts;
    std::map<std::string, std::set<uint32_t>> sets;
    // Native GUIDs stay integer-valued; policies use only named target references.
    std::map<std::string, uint64_t> targets;
};
enum Resource : uint32_t { None = 0, Movement = 1, Casting = 2, Interaction = 4 };
enum class ActionKind { Cast, Move, Interact, Wait };
struct Ability {
    ActionKind kind = ActionKind::Cast;
    uint32_t spell = 0, resources = Casting;
    std::set<std::string> targets;
};
struct CapabilityCatalog {
    uint32_t version = 1;
    std::map<std::string, Ability> abilities;
};
// Three first-rank spell examples for foundation validation, not a production rotation.
CapabilityCatalog FoundationCapabilities();
struct Action {
    std::string ability, target;
};
struct Intent {
    Scope scope;
    uint64_t observation = 0, policyGeneration = 0, issuedMs = 0, expiresMs = 0;
    std::string encounter;
    uint64_t encounterAttempt = 0, encounterEpoch = 0;
    std::string owner, rule;
    Action action;
    uint64_t targetGuid = 0;
    int64_t priority = 0;
    uint32_t resources = None;
    bool interruptible = true;
};
// Diagnostic data is bounded and contains IDs, never account credentials or chat.
struct DecisionEvent {
    Scope scope;
    uint64_t observation = 0, policyGeneration = 0, timeMs = 0;
    std::string owner, rule, stage, reason;
    std::string encounter;
    uint64_t encounterAttempt = 0, encounterEpoch = 0;
};
class EventBuffer {
public:
    explicit EventBuffer(size_t capacity = 256);
    void Push(DecisionEvent event);
    std::vector<DecisionEvent> Drain();
    uint64_t Dropped() const { return m_dropped; }
private:
    size_t m_capacity;
    uint64_t m_dropped = 0;
    std::deque<DecisionEvent> m_events;
};
Result Validate(Intent const&, Observation const& current);
// Thread-confined per actor. Leases describe ownership, not cancellation of native actions.
// Only Dispatch's executor commit can apply a preemption to the actual game world.
class Arbiter {
public:
    void Reset(Scope const&);
    Result Acquire(Intent const&, Observation const& current);
    void Release(std::string const& owner);
    std::vector<Intent> Active(uint64_t nowMs);
private:
    Scope m_scope;
    std::string m_encounter;
    uint64_t m_attempt = 0, m_epoch = 0;
    bool m_initialized = false;
    std::vector<Intent> m_leases;
};
// Adapters must perform the final native legality check on the server's safe thread.
// Check cannot mutate game state and MUST verify resources/targets against its trusted
// capability catalog. Execute must revalidate before any native side effect, and commit
// cancellation of displaced actions with the new action. A failed Execute means NO change
// was committed, including no cancellation. Adapters unable to guarantee this must reject.
class Executor {
public:
    virtual ~Executor() = default;
    virtual Result Check(Intent const&, Observation const& current) = 0;
    virtual Result Execute(Intent const&, Observation const& current,
                           std::vector<Intent> const& displaced) = 0;
};
// Bounded attempts; shadow never calls Execute or acquires/replaces real leases.
Result Dispatch(std::vector<Intent> const&, Observation const&, Arbiter&, Executor&,
                EventBuffer&, bool shadow, size_t maxAttempts = 8);
}
#endif
