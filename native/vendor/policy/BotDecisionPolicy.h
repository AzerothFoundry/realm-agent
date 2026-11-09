// GPL-2.0-or-later.
#ifndef MANGOS_BOT_DECISION_POLICY_H
#define MANGOS_BOT_DECISION_POLICY_H
#include "BotDecision.h"
#include <memory>
#include <mutex>
namespace BotDecision { namespace PolicyRules {
struct Condition {
    std::string field, op, text;
    ValueType type = ValueType::Number;
    double number = 0;
    bool flag = false;
    uint32_t id = 0;
};
enum class Truth { False, True, Unknown };
struct Expression {
    enum class Kind { Leaf, All, Any, Not };
    Kind kind = Kind::All;
    Condition condition;
    std::vector<Expression> children;
};
// Unknown stays unknown through negation; absent observations never grant permission.
Truth Evaluate(Expression const&, Observation const&);
struct Adjustment { std::vector<Condition> when; int score = 0; Expression expr; };
struct Rule {
    std::string id, owner;
    bool enabled = true;
    int priority = 0;
    uint32_t ttlMs = 500;
    std::vector<Condition> when, any;
    std::vector<Adjustment> adjust;
    Expression whenExpr;
    Action action;
    uint32_t resources = None;
};
struct Policy {
    uint32_t schemaVersion = 1;
    uint64_t revision = 0, generation = 0;
    bool enabled = false;
    std::vector<Rule> rules;
    std::map<std::string, Expression> predicates;
};
Result Parse(std::string const&, FieldCatalog const&, CapabilityCatalog const&, Policy&);
struct RuleTrace { std::string id; bool eligible = false, unknown = false; std::string reason; };
std::vector<RuleTrace> Trace(Policy const&, Observation const&, size_t maxRules = 64);
std::vector<Intent> Rank(Policy const&, Observation const&, size_t maxCandidates = 8);
class Store {
public:
    Store(FieldCatalog fields, CapabilityCatalog capabilities);
    Result Stage(std::string const&);
    Result PublishAtBoundary();
    Result ReloadNow(std::string const&, bool safeBoundary);
    std::shared_ptr<Policy const> Current() const;
    std::shared_ptr<Policy const> Pending() const;
    std::string Status() const;
private:
    FieldCatalog m_fields;
    CapabilityCatalog m_capabilities;
    mutable std::mutex m_mutex;
    std::shared_ptr<Policy const> m_active, m_pending;
    uint64_t m_generation = 0;
};
}}
#endif
