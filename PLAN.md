# Headless task client: initial scope

User-approved scope: independent native 1.12.1 client, any selected mage or warrior character; status and task controls, SSE events and local bounded JSONL; complete city duel task with local navigation, supplies, preparation, duel, recovery and repetition. Two dedicated test characters on separate accounts for concurrent observation.

Use shared value-only decision source from AzerothFoundry/vmangos-core and derive supported combat rules from existing server PVP policy. Client observations and native packet actions are separate adapters; missing information remains unknown. No server state/Map pointers, GM commands, new game protocol or schema dependency in the runtime.

Local MMap/Detour navigation. Do not teleport by changing coordinates or send speed above native allowance. Mage city return uses learned native teleport and actual reagent; warrior walks when map is reachable. Unknown routes/cross-map transitions are explicit blocked states. Local map data is user-provided, not published.

Live verification uses only newly provisioned dedicated fixture accounts/characters. Setup may prepare these fixtures, but the runtime credentials have ordinary player permissions and all task actions must be native player requests. Never operate a real user's active character.

Validation must distinguish pure FSM tests, HTTP/SSE contract tests, real navigation geometry/path traversal, and actual game sessions. Do not report a sent cast/purchase as success before authoritative state confirms it. No automatic production server restart.
