# Strategy policy

`src/policy.rs` is the client-side policy foundation. Policies are strict JSON, versioned, bounded, and loaded as immutable snapshots. Rules are scoped by mode and opponent count, grouped into phases, and rank deterministically by priority then rule ID. Missing facts are `Unknown`; negation never turns missing data into permission. Actions name registered capabilities and do not contain raw network or GM operations.

`Store::stage` parses a candidate without changing the active snapshot; `publish` changes it at an explicit boundary and increments the generation. A combat adapter should pin the active `Arc<Policy>` until the encounter ends. Invalid JSON keeps the last good policy.

The current engine is an offline foundation. It is not yet connected to the live protocol loop and does not claim complete PVP/PVE rotations.
