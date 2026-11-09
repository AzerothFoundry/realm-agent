// GPL-2.0-or-later. Policies name registered capabilities; JSON never grants execution privileges.
#include "BotDecisionPolicy.h"
#include <nlohmann/json.hpp>
#include <algorithm>
#include <cmath>
#include <limits>
#include <stdexcept>
namespace BotDecision { namespace PolicyRules { namespace {
using J = nlohmann::json;
void Need(bool ok, char const* why) { if (!ok) throw std::runtime_error(why); }
void Keys(J const& j, std::initializer_list<char const*> keys) {
    Need(j.is_object(), "expected object");
    for (auto it=j.begin(); it!=j.end(); ++it) {
        bool found=false; for (auto k:keys) if(it.key()==k) found=true;
        Need(found,"unknown key");
    }
}
std::string Text(J const& j) {
    Need(j.is_string(),"expected string"); auto s=j.get<std::string>();
    Need(!s.empty() && s.size()<=128,"invalid string length");
    for (unsigned char c:s) Need(c>=32 && c!=127,"control character in string");
    return s;
}
double Number(J const& j) { Need(j.is_number(),"expected number"); double x=j.get<double>();Need(std::isfinite(x) && std::fabs(x)<=1e12,"invalid number");return x; }
uint64_t UInt(J const& j,uint64_t hi) {
    Need(j.is_number_integer(),"expected integer");
    if(j.is_number_unsigned()){auto n=j.get<uint64_t>();Need(n<=hi,"integer out of range");return n;}
    auto n=j.get<int64_t>();Need(n>=0 && uint64_t(n)<=hi,"integer out of range");return uint64_t(n);
}
int Score(J const& j) {Need(j.is_number_integer(),"expected integer score");auto n=Number(j);Need(n>=-100000 && n<=100000,"score out of range");return int(n);}
Condition ReadCondition(J const& j,FieldCatalog const& fields) {
    Keys(j,{"field","op","value"});Condition c;c.field=Text(j.at("field"));c.op=Text(j.at("op"));
    auto it=fields.fields.find(c.field);Need(it!=fields.fields.end(),"unregistered field");c.type=it->second;auto const& v=j.at("value");
    if(c.type==ValueType::Number) {Need(c.op=="eq"||c.op=="ne"||c.op=="lt"||c.op=="lte"||c.op=="gt"||c.op=="gte","invalid numeric operator");c.number=Number(v);}
    if(c.type==ValueType::Flag){Need(c.op=="eq"||c.op=="ne","invalid flag operator");Need(v.is_boolean(),"expected flag");c.flag=v.get<bool>();}
    if(c.type==ValueType::Text){Need(c.op=="eq"||c.op=="ne","invalid text operator");c.text=Text(v);}
    if(c.type==ValueType::IdSet){Need(c.op=="contains"||c.op=="not_contains","invalid set operator");c.id=uint32_t(UInt(v,UINT32_MAX));}
    return c;
}
std::vector<Condition> Conditions(J const& j,FieldCatalog const& fields) {Need(j.is_array()&&j.size()<=32,"too many conditions");std::vector<Condition> out;for(auto const& c:j)out.push_back(ReadCondition(c,fields));return out;}
Expression ReadExpression(J const& j, FieldCatalog const& fields, J const& predicates,
                          std::set<std::string>& visiting, unsigned depth, unsigned& nodes) {
    Need(depth <= 8 && ++nodes <= 256, "expression budget exceeded");
    Need(j.is_object(), "expected expression object");
    Expression e;
    if (j.count("field")) { e.kind=Expression::Kind::Leaf; e.condition=ReadCondition(j,fields); return e; }
    Need(j.size()==1, "expression requires one operator");
    if (j.count("ref")) {
        auto name=Text(j.at("ref")); Need(predicates.count(name)!=0, "missing predicate reference");
        Need(visiting.insert(name).second, "predicate reference cycle");
        e=ReadExpression(predicates.at(name),fields,predicates,visiting,depth+1,nodes);
        visiting.erase(name); return e;
    }
    if (j.count("not")) {
        e.kind=Expression::Kind::Not;
        e.children.push_back(ReadExpression(j.at("not"),fields,predicates,visiting,depth+1,nodes));
        return e;
    }
    bool all=j.count("all")!=0; Need(all || j.count("any"), "unknown expression operator");
    auto const& list=j.at(all?"all":"any"); Need(list.is_array()&&!list.empty()&&list.size()<=32,"invalid expression list");
    e.kind=all?Expression::Kind::All:Expression::Kind::Any;
    for(auto const& child:list)e.children.push_back(ReadExpression(child,fields,predicates,visiting,depth+1,nodes));
    return e;
}
Expression ReadExpression(J const& j,FieldCatalog const& fields,J const& predicates,unsigned& expandedNodes) {
    std::set<std::string> visiting; unsigned nodes=0;
    auto expression=ReadExpression(j,fields,predicates,visiting,0,nodes);
    expandedNodes+=nodes;Need(expandedNodes<=16384,"policy expanded expression budget exceeded");
    return expression;
}
bool Match(Condition const& c,Observation const& o) {
    switch(c.type){
    case ValueType::Number:{auto i=o.numbers.find(c.field);if(i==o.numbers.end()||!std::isfinite(i->second))return false;double x=i->second,y=c.number;
        if(c.op=="eq")return x==y;if(c.op=="ne")return x!=y;if(c.op=="lt")return x<y;if(c.op=="lte")return x<=y;if(c.op=="gt")return x>y;return x>=y;}
    case ValueType::Flag:{auto i=o.flags.find(c.field);return i!=o.flags.end()&&(c.op=="eq"?(i->second==c.flag):(i->second!=c.flag));}
    case ValueType::Text:{auto i=o.texts.find(c.field);return i!=o.texts.end()&&(c.op=="eq"?(i->second==c.text):(i->second!=c.text));}
    case ValueType::IdSet:{auto i=o.sets.find(c.field);return i!=o.sets.end()&&(c.op=="contains"?i->second.count(c.id)!=0:i->second.count(c.id)==0);}
    }return false;
}
bool All(std::vector<Condition> const& cs,Observation const& o){return std::all_of(cs.begin(),cs.end(),[&](Condition const& c){return Match(c,o);});}
}
Truth Evaluate(Expression const& e, Observation const& o) {
    if(e.kind==Expression::Kind::Leaf){
        auto const& c=e.condition; bool exists=false;
        switch(c.type){
        case ValueType::Number: {auto i=o.numbers.find(c.field);exists=i!=o.numbers.end()&&std::isfinite(i->second);break;}
        case ValueType::Flag: exists=o.flags.count(c.field)!=0;break;
        case ValueType::Text: exists=o.texts.count(c.field)!=0;break;
        case ValueType::IdSet: exists=o.sets.count(c.field)!=0;break;
        }
        if(!exists)return Truth::Unknown;
        return Match(c,o)?Truth::True:Truth::False;
    }
    if(e.kind==Expression::Kind::Not){
        if(e.children.size()!=1)return Truth::Unknown;
        auto t=Evaluate(e.children.front(),o);
        return t==Truth::Unknown?t:t==Truth::True?Truth::False:Truth::True;
    }
    bool unknown=false;
    for(auto const& child:e.children){auto t=Evaluate(child,o);
        if(e.kind==Expression::Kind::All&&t==Truth::False)return Truth::False;
        if(e.kind==Expression::Kind::Any&&t==Truth::True)return Truth::True;
        unknown=unknown||t==Truth::Unknown;
    }
    if(unknown)return Truth::Unknown;
    return e.kind==Expression::Kind::All?Truth::True:Truth::False;
}
Result Parse(std::string const& text,FieldCatalog const& fields,CapabilityCatalog const& capabilities,Policy& out) {
    try {
        Need(text.size()<=1024*1024,"policy exceeds 1 MiB");std::vector<std::set<std::string>> stack;
        J j=J::parse(text,[&](int depth,J::parse_event_t event,J& value){Need(depth<=24,"policy nesting too deep");if(event==J::parse_event_t::object_start)stack.emplace_back();if(event==J::parse_event_t::key)Need(stack.back().insert(value.get<std::string>()).second,"duplicate JSON key");if(event==J::parse_event_t::object_end)stack.pop_back();return true;});
        Keys(j,{"schema_version","revision","enabled","rules","predicates"});Policy p;
        Need(UInt(j.at("schema_version"),1)==1,"unsupported schema");p.revision=UInt(j.at("revision"),9007199254740991ULL);
        Need(j.at("enabled").is_boolean(),"expected enabled flag");p.enabled=j.at("enabled").get<bool>();
        J predicates=j.count("predicates")?j.at("predicates"):J::object();
        Need(predicates.is_object()&&predicates.size()<=64,"too many predicates");unsigned expandedNodes=0;
        for(auto it=predicates.begin();it!=predicates.end();++it){Text(J(it.key()));p.predicates[it.key()]=ReadExpression(it.value(),fields,predicates,expandedNodes);}
        auto const& rules=j.at("rules");Need(rules.is_array()&&rules.size()<=512,"too many rules");std::set<std::string> ids;
        for(auto const& v:rules){
            Keys(v,{"id","owner","enabled","priority","ttl_ms","when","any","when_expr","adjust","action"});Rule r;r.id=Text(v.at("id"));Need(ids.insert(r.id).second,"duplicate rule id");r.owner=Text(v.at("owner"));r.priority=Score(v.at("priority"));
            if(v.count("enabled")){Need(v.at("enabled").is_boolean(),"invalid rule enabled");r.enabled=v.at("enabled").get<bool>();}
            if(v.count("ttl_ms")){r.ttlMs=uint32_t(UInt(v.at("ttl_ms"),5000));Need(r.ttlMs>0,"zero TTL");}
            if(v.count("when"))r.when=Conditions(v.at("when"),fields);if(v.count("any"))r.any=Conditions(v.at("any"),fields);
            if(v.count("when_expr"))r.whenExpr=ReadExpression(v.at("when_expr"),fields,predicates,expandedNodes);
            if(v.count("adjust")){auto const& a=v.at("adjust");Need(a.is_array()&&a.size()<=16,"too many adjustments");for(auto const& x:a){Keys(x,{"when","expr","add"});Need(x.count("when")||x.count("expr"),"adjustment requires conditions");
                Adjustment a;a.score=Score(x.at("add"));if(x.count("when"))a.when=Conditions(x.at("when"),fields);if(x.count("expr"))a.expr=ReadExpression(x.at("expr"),fields,predicates,expandedNodes);r.adjust.push_back(std::move(a));}}
            auto const& a=v.at("action");Keys(a,{"ability","target"});r.action={Text(a.at("ability")),Text(a.at("target"))};
            auto cap=capabilities.abilities.find(r.action.ability);Need(cap!=capabilities.abilities.end(),"unregistered ability");Need(cap->second.targets.count(r.action.target)!=0,"unsupported ability target");
            Need((cap->second.resources&~uint32_t(Movement|Casting|Interaction))==0,"invalid capability resources");r.resources=cap->second.resources;p.rules.push_back(std::move(r));
        }
        out=std::move(p);return {true,"parsed"};
    } catch(std::exception const& e){return {false,e.what()};}
}
std::vector<RuleTrace> Trace(Policy const& p,Observation const& o,size_t maxRules) {
    std::vector<RuleTrace> out;
    for(auto const& r:p.rules){
        if(out.size()>=std::min<size_t>(maxRules,512))break;
        RuleTrace t;t.id=r.id;
        if(!p.enabled||!r.enabled)t.reason="disabled";
        else if(!o.scope.actor||!o.sequence)t.reason="invalid_observation";
        else {
            Expression all;all.children.push_back(r.whenExpr);
            for(auto const& c:r.when){Expression leaf;leaf.kind=Expression::Kind::Leaf;leaf.condition=c;all.children.push_back(leaf);}
            if(!r.any.empty()){Expression any;any.kind=Expression::Kind::Any;for(auto const& c:r.any){Expression leaf;leaf.kind=Expression::Kind::Leaf;leaf.condition=c;any.children.push_back(leaf);}all.children.push_back(std::move(any));}
            auto truth=Evaluate(all,o);
            if(truth==Truth::Unknown){t.reason="unknown_observation";t.unknown=true;}
            else if(truth==Truth::False)t.reason="condition_false";
            else if(!o.targets.count(r.action.target)||!o.targets.at(r.action.target))t.reason="missing_target";
            else if(o.nowMs>std::numeric_limits<uint64_t>::max()-r.ttlMs)t.reason="time_overflow";
            else {t.reason="matched";t.eligible=true;}
        }
        out.push_back(std::move(t));
    }
    return out;
}
std::vector<Intent> Rank(Policy const& p,Observation const& o,size_t maxCandidates){
    std::vector<Intent> out;if(!p.enabled||!o.scope.actor||!o.sequence)return out;
    for(auto const& r:p.rules){
        if(!r.enabled||Evaluate(r.whenExpr,o)!=Truth::True||!All(r.when,o)||(!r.any.empty()&&!std::any_of(r.any.begin(),r.any.end(),[&](Condition const& c){return Match(c,o);})))continue;
        auto target=o.targets.find(r.action.target);if(target==o.targets.end()||!target->second)continue;
        if(o.nowMs>std::numeric_limits<uint64_t>::max()-r.ttlMs)continue;
        Intent i;i.encounter=o.encounter;i.encounterAttempt=o.encounterAttempt;i.encounterEpoch=o.encounterEpoch;i.scope=o.scope;i.observation=o.sequence;i.policyGeneration=p.generation;i.issuedMs=o.nowMs;i.expiresMs=o.nowMs+r.ttlMs;i.owner=r.owner;i.rule=r.id;i.action=r.action;i.targetGuid=target->second;i.resources=r.resources;i.priority=r.priority;
        for(auto const& a:r.adjust)if(All(a.when,o)&&Evaluate(a.expr,o)==Truth::True)i.priority+=a.score;out.push_back(std::move(i));
    }
    std::sort(out.begin(),out.end(),[](Intent const& a,Intent const& b){return a.priority!=b.priority?a.priority>b.priority:a.rule<b.rule;});
    if(out.size()>std::min<size_t>(maxCandidates,64))out.resize(std::min<size_t>(maxCandidates,64));return out;
}
Store::Store(FieldCatalog fields,CapabilityCatalog capabilities):m_fields(std::move(fields)),m_capabilities(std::move(capabilities)),m_active(std::make_shared<Policy>()){}
Result Store::Stage(std::string const& text){Policy p;auto result=Parse(text,m_fields,m_capabilities,p);if(!result.ok)return result;std::lock_guard<std::mutex> lock(m_mutex);m_pending=std::make_shared<Policy const>(std::move(p));return {true,"staged; active unchanged"};}
Result Store::PublishAtBoundary(){std::lock_guard<std::mutex> lock(m_mutex);if(!m_pending)return {false,"no pending policy"};Policy p=*m_pending;p.generation=++m_generation;m_active=std::make_shared<Policy const>(std::move(p));m_pending.reset();return {true,"published at caller-certified safe boundary"};}
Result Store::ReloadNow(std::string const& text,bool safeBoundary){if(!safeBoundary)return {false,"unsafe boundary"};Policy p;auto result=Parse(text,m_fields,m_capabilities,p);if(!result.ok)return result;std::lock_guard<std::mutex> lock(m_mutex);p.generation=++m_generation;m_active=std::make_shared<Policy const>(std::move(p));m_pending.reset();return {true,"published"};}
std::shared_ptr<Policy const> Store::Current()const{std::lock_guard<std::mutex> lock(m_mutex);return m_active;}
std::shared_ptr<Policy const> Store::Pending()const{std::lock_guard<std::mutex> lock(m_mutex);return m_pending;}
std::string Store::Status()const{std::lock_guard<std::mutex> lock(m_mutex);return "generation="+std::to_string(m_active->generation)+" revision="+std::to_string(m_active->revision)+" pending="+(m_pending?"1":"0");}
}}
