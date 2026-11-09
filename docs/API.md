# realm-agent API

The agent binds to loopback by default (`127.0.0.1:18112`) and requires a Bearer token of at least 16 characters. It never accepts a public bind address. Commands are queued; an HTTP `202` means queued, not executed.

- `GET /status` returns the character, position, current task phase, health/mana and visible object count.
- `POST /tasks` accepts a strict `TaskSpec` JSON body. The task must name a site and an explicit opponent GUID allowlist.
- `POST /task/pause`, `/task/resume`, `/task/cancel` control the current task.
- `GET /events/history?after=N` returns bounded events and a gap marker when the cursor is too old.
- `GET /events` is an SSE stream. Send `Last-Event-ID` to resume; a `gap` event means history must be recovered with the history endpoint.
- `POST /shutdown` requests a safe client shutdown.

Events are local JSONL with a bounded ring and rotation. Passwords, session keys, tokens, raw packets and raw chat are rejected from event detail. A newly created session file is required so an old log cannot be overwritten.

The first task state machine supports only a selected level-60 Mage or Warrior. Mage conjures food/water when learned spells and inventory confirm it; Warrior visits the configured inn when food is insufficient. Both navigate by local MMap/Detour data, challenge only allowlisted opponents and wait for authoritative duel packets before counting a duel. No GM command, database write, coordinate teleport or speed modification is used.

The protocol transport and character login wiring remain a separate implementation step. Until that adapter is connected, the API and task tests are offline-only and must not be described as a live game session.
