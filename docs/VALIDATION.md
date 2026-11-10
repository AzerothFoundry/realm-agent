# Online task validation — 2026-11-10

## Verified

A dedicated Horde female Troll Mage, Ada, logged in through the actual `realm-agent` executable and `benilla-protocol`, not the earlier temporary login holder. Only the selected dedicated account was used. The server was not restarted, no schema changed, and no GM command was sent by the runtime.

The live sequence included:

1. Login/character selection, actor object fields, 240 known spells, health/mana and inventory synchronization.
2. Four native conjure requests; inventory changes confirmed food and water before travel.
3. Local MMap/Detour route from inside Orgrimmar toward the gate PK area, normal-speed movement packets and server-side saved-position readback near the destination.
4. Buff state confirmation and recovery, idle allowlisted opponent selection, challenge request, actual server duel request/countdown.
5. Native Frost Nova and high-rank Frostbolt requests, server cast failures correctly recorded, actual duel completion/result.
6. Recovery to full HP/MP and continued opponent selection; the final root-ACK-corrected session completed at least two duels and continued the same hourly task.
7. Root group membership appeared in the authoritative server group roster and `/status`. Automatic acceptance code and task-preservation tests pass. This does not claim a fresh invite-to-join E2E occurred during this particular final session, because the group already existed.
8. Actual HTTP status/object pagination, authenticated WebSocket upgrade and received `state.snapshot` on the live Ada service.

The client is installed as a macOS user LaunchAgent, with ordinary player game actions. Credentials/configuration/logs remain in private Application Support files, not the repository. The service restarts the actor on failure; the game server is unaffected. Authentication fails safely if another session is using the same account. Do not simultaneously log that account in through another client.

## Failures found and corrected

- A blocked network read originally stopped task ticks; bounded reader channel and independent actor ticks now separate network arrival from movement/control.
- The initial pose must also come from the selected roster/self object creation; a login-only position packet is not sufficient.
- Remote Bot movement is commonly `MonsterMove`/create spline; ignoring it left opponent positions stale. A bounded spline interpolator now updates estimates and acknowledges server-driven self splines.
- `Root` movement ACK requires `0x1000`, as in the 1.12 client. Missing that bit produced server movement errors/disconnects. ACK, stop/facing and resting heartbeat now preserve it; the root-corrected live session continued across duels. No server rule was weakened.
- A real native Ping every 30 seconds is required in addition to movement heartbeats.
- Challenge result 104 means target already dueling. Public duel-arbiter fields and target backoff prevent repeatedly challenging busy participants.
- Buying/creating/sending requests is never treated as received stock; authoritative inventory confirms supplies.

## Memory observations

Earlier live process samples were around 6 MiB RSS before navigation and approximately 37 MiB while loading/querying local navigation. Subsequent stationary/combat samples in the root-corrected process were approximately 6–37 MiB (including later navigation queries). These are short-run RSS observations on this Mac, not a leak proof, fixed memory guarantee or multi-agent capacity result. The actor samples its own RSS every 30 seconds into bounded local events and exposes `resident_kib` in status.

## Automated checks

`cargo test --all-targets` passes: four library/navigation/spline tests, six API tests (including actual loopback WS TCP), three policy parser tests, three policy runtime tests, six strategy tests and fourteen task tests. Release build succeeds on macOS ARM64. Existing vendored Detour emits a compiler warning about its historical memset implementation.

## Remaining boundaries

- The one-hour boundary and deferred rotation during an active duel are tested using simulated time. A complete uninterrupted real hour was not awaited for this report.
- The task candidate pool currently contains only city_duel. Reconnect starts a new task generation; durable cross-process task continuation is not implemented.
- This is a minimal combat policy, not a guarantee of winning. World PVE and the warrior branches have tests but did not receive equivalent live-role validation in this run.
- No transport, taxi, jumping, swimming, dynamic-door or ghost/death recovery route was tested. Local straight-path following is not the complete graphical client's terrain/collision physics.
- New server actions and fields are still added through the protocol adapter. The runtime has no WoW addon/Lua engine.
- JSON directory precedence works, but named-policy transitions, all-class rotations and complete encounter phase graphs remain later work.
