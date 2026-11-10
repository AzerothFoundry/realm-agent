# Strategy policy

`src/policy.rs` is the client-side policy foundation. Policies are strict JSON, versioned, bounded, and loaded as immutable snapshots. Rules are scoped by mode and opponent count, grouped into phases, and rank deterministically by priority then rule ID. Missing facts are `Unknown`; negation never turns missing data into permission. Actions name registered capabilities and do not contain raw network or GM operations.

`Store::stage` parses a candidate without changing the active snapshot; `publish` changes it at an explicit boundary and increments the generation. A combat adapter should pin the active `Arc<Policy>` until the encounter ends. Invalid JSON keeps the last good policy.

`Store::reload_file(path)` is the file based hot-reload entry point: it reads and parses the complete file, then publishes only after validation. Call it from one owner at a bounded interval (for example once per second), and keep the returned `Arc<Policy>` pinned for an active encounter. Atomic file replacement is recommended so a reader never sees an editor's partial write.

The engine is wired into the live task loop through PolicyRuntime. The small example rotations do not claim complete PVP/PVE behavior for every class or encounter.

## Online task adapter

`PolicyRuntime` now loads one file or a directory of JSON files (lexicographic filename order), at most once per second when combat decisions are evaluated. It publishes a complete valid bundle atomically, keeps the last good version on error, pins the old bundle during a fight and releases it on duel completion. An invalid first configuration fails initialization. It maps named supported abilities to the highest learned rank and then to normal internal `Action` values; no raw protocol endpoint is exposed.

The task currently calls only the `engage` phase. Other phase names and `min_hold_ms` are representable but do not form an automatic phase-transition graph yet. Directory order selects policy order; automatic named policy transitions and per-file fault isolation are not complete. The included PVE/PVP files are deliberately small example rotations, not competitive strategies for every opponent.

`None` from the adapter means legacy fallback, including no applicable JSON rule; explicit `decision.wait` yields an intentional empty action list. State reports the selected rule and current generation/error. Enemy cooldowns remain unknown unless observed, never fetched from a server database.
