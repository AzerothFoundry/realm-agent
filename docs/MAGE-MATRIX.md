# Client Mage matchup migration: nine independent policies

This is a **partial migration of existing server rules and the project's 1.12.1 Mage whitepaper into executable client policies**, not proof of an optimal nine-class duel AI. The old single `50-city-pvp.json` was removed. Each matchup file requires both `self.class == 8` and the exact observed opponent class. Unknown classes do not match. Common emergency defenses live in `45-mage-common.json`; basic Warrior rules remain in `60-warrior-common.json` and are not a Warrior matchup matrix.

## Sources actually inspected

- Server `config/pvp-combat-policy.example.json`: the nine class-8 rules (`mage-ice-block`, barrier, counterspell, nova, fireblast, cone, frostbolt, fireball, missiles). Priorities, health threshold decisions and observed-cast guards inspired this migration; source spell IDs are not blindly copied as all effects, rank normalization and transport support must be checked.
- Server `src/game/PlayerBots/BattleBotAI.cpp:2755`: `UpdateInCombatAI_Mage`; observed cast Counterspell near 2780, movement/close-range pressure following 2800, dispel and Polymorph near 2863–2880. The original secondary-target selection contains tightly coupled native state and is not copied as client omniscience.
- Local project whitepaper `PVP职业白皮书/法师.md`, sections M-WAR/PAL/HUN/ROG/PRI/SHA/MAG/LOC/DRU. This document was inspected as project strategy material, not as fresh verification of every external guide it cites. Its Classic Era references are not treated as exact 1.12 data. No new external guide was fetched for this migration.

## Effective file and tactical difference

| File | Concrete client-supported differences | Explicit remaining gaps |
| --- | --- | --- |
| 50 mage vs warrior | Nova close approach, open distance after observed root, Cone and instant Fireblast under close pressure | No inferred Intercept/insignia cooldown, no terrain-safe Blink |
| 51 mage vs rogue | Nova/Cone at contact, Fireball prioritized while visible at range to discourage re-stealth | No targeting vanished rogue; no trap-like ground sweep or stun-break Blink |
| 52 mage vs warlock | Actual observed Fear IDs get interrupt priority; ordinary observed casts still eligible; Remove Curse only for enumerated known aura IDs | No pet school-lock/cooldown guess, no exhaustive curse taxonomy or pet-priority engine |
| 53 mage vs priest | Counterspell observed casts, instant Fireblast near fear range before ordinary frost damage | No fake school-lock inference; not all casts identified as heals |
| 54 mage vs hunter | Higher contact Nova, Cone and instant damage in 5–8 yard shooting dead-zone interval | No invisible trap knowledge, no reliable trap-owner observation; exact hitbox dead-zone remains native authority |
| 55 mage vs shaman | Counterspell observed cast, Cone contact priority, instant damage just outside melee | Grounding/Tremor owner classification and totem target switching are not implemented |
| 56 mage vs druid | Observed humanoid form permits low-health Polymorph, then holds damage while sheep is observed; contact Nova/Cone; Fireball at distance | No Polymorph when form observation missing or shifted; hold does not yet trigger a full heal/drink reset; no inferred shapeshift cooldown |
| 57 mage vs paladin | Stop damaging visible Divine Shield, interrupt observed casts; larger instant finish window | No assumption that physical protection is spell immunity; Freedom counterplay and healing-state classification incomplete |
| 58 mage vs mage | Stop damage into observed Ice Block; observed cast interrupt; Fireball into observed Frost Ward | No hidden Counterspell/Blink cooldown knowledge or speculative school lock |

All matchups retain a class-guarded Frostbolt baseline and long-distance approach. Shared baseline rules are deliberately reused; the distinct branches above, not renamed copies of the baseline, are the scope of this migration. `combat.approach` is only a desired standoff point; actual movement must pass local navigation. No instant coordinate relocation, charge or Blink path is introduced.

## Observation and version boundary

Observed aura IDs, class, form and current cast come from the protocol mirror only. Enemy cast history is not equivalent to a known remaining cooldown. Missing fields remain Unknown through negation. Remove Curse uses known explicit Curse aura examples and does not claim all curse effects can be inferred from an arbitrary spell name. Polymorph requires an actually learned rank and targets a visibly humanoid Druid only in the configured reset case.

Policies compete by global priority across files, with deterministic ties. Combat policy snapshots remain pinned until a combat boundary; changing JSON does not mutate an active fight's decisions. `/status` should expose the selected file and rule. This migration has offline policy tests; online win rates, full control chains, diminishing returns and all nine matchups remain unverified.

## Observed-event reactions

Recent visible Recklessness can elevate Barrier against a Warrior when health is below 50%; recent visible Kidney Shot can elevate Barrier against a Rogue. Recent visible Counterspell can prefer an instant Fireblast over a long cast in Mage mirror. `target.recent_spells` is a five-second observation window, **not** proof of remaining enemy cooldown, school lock or counterspell availability. These bounded event reactions are configuration heuristics, not matchup-optimal claims. If the event was not observed, the reaction does not activate.
