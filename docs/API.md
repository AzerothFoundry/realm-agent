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

The protocol transport and character login are connected to the main actor loop. The API contract tests are still offline tests; separate live Ada session evidence is recorded in VALIDATION.md.

## WebSocket and visible objects

`GET /ws?after=N` upgrades to an authenticated read-only WebSocket event stream. Use the same Authorization Bearer header as HTTP; tokens are never accepted in the URL. The first message is `state.snapshot`, followed by replayed and live `event` envelopes. IDs are monotonically increasing within a session. `gap` explicitly indicates a cursor/history or subscriber lag; recover through `/events/history` and refresh `/status`. Client text/binary commands are rejected with an error envelope; task control remains HTTP-only. Frames/messages are limited to 16 KiB. Existing `/events` SSE remains supported.

`GET /world/objects?offset=0&limit=100` returns a page from the latest bounded object snapshot. `limit` must be 1–200. `count` is the actual synchronized count, `available` the retained snapshot length, `partial` signals truncation, and `next` is the next available offset or null. A snapshot may hold at most 512 rows, so paging does not imply access to every object when `partial` is true.

Validation includes a real loopback TCP WebSocket handshake, missing-token rejection, initial snapshot, replay, live event, and rejection of a raw-action text message. These API tests do not prove game protocol synchronization or live combat behavior.
