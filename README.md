# realm-agent

Native headless WoW 1.12.1 task client. A selected Mage or Warrior uses the normal login/world protocol, local MMap navigation and client-owned tasks. No window, rendering, Lua addon runtime or server GM command is needed.

## Run

```sh
cargo build --release --bin realm-agent
export REALM_AGENT_PASSWORD='your-test-account-password'
export REALM_AGENT_API_TOKEN='your-local-api-token-at-least-16-characters'
./target/release/realm-agent --config config/agent.example.json
```

Edit the config's server/account/character and local MMAP path first. Paths are relative to the config file. CLI `--host`, `--username`, `--character` and `--api` override these fields. Secrets are read from named environment variables, never API snapshots or log records. Do not put them in command line arguments. The HTTP/WS listener is loopback-only and starts after authentication and character selection succeed.

`task_file` is optional: omit it to log in with status/events and automatic group acceptance but no autonomous task. With a task file, the actor starts after its own state and spellbook arrive. Never log the same account into another client simultaneously.

## City duel task

The one-hour task checks supplies, conjures food/water as a Mage (or visits a configured inn/vendor), navigates to the configured arena, prepares buffs, selects an idle allowlisted player, challenges and waits for native duel countdown/completion, then recovers and continues. Supplies are replenished when exhausted, not after each duel. At an hour boundary the current duel is allowed to finish before selecting the next task; the current task pool contains only `city_duel`. Pausing/cancelling stops the current task promptly, including forfeiting an active duel.

Example opponent GUIDs are placeholders: replace them with consenting test players/bots on your realm. A server does not normally identify a player as a Bot to clients, so runtime does not guess or challenge arbitrary humans. It reads the public duel arbiter and combat state to avoid challenging busy opponents. Native refusal is recorded and that opponent is backed off.

Group invites are accepted when enabled, independent of whether a task is running; the server roster confirms actual membership. Grouping never activates follow behavior or replaces the task goal. This accepts invitations sent by the game server, not arbitrary websocket commands.

## Control and events

All routes require `Authorization: Bearer ...`:

- `GET /status`: account/character, position, HP/MP, task, navigation, policy, group, spellbook/inventory counts, resident memory and a bounded spatial-object snapshot.
- `GET /world/objects?offset=0&limit=100`: paginated object snapshots; `partial` reports truncation.
- `POST /tasks`: start a task using its JSON specification.
- `POST /task/pause`, `/task/resume`, `/task/cancel` and `/shutdown`.
- `GET /ws?after=0`: WebSocket initial snapshot, replay and live events with explicit gaps; receive-only.
- `GET /events` and `/events/history?after=0`: legacy SSE and bounded event recovery.

HTTP 202 means queued, not completed; use status and events to observe the result. WebSocket messages cannot request raw movement, casts or packets. See [API](docs/API.md), [policy](docs/POLICY.md), [Mage matchup matrix](docs/MAGE-MATRIX.md), and [live validation](docs/VALIDATION.md).

## Limits

This is a first online vertical slice, not a full replacement client or complete AI for every class. Native position interpolation, local path following, cast hints and protocol replies are implemented; ultimate action legality is decided by the realm. The world view is limited to received objects, not omniscient server state.

Navigation uses matching user-provided vMaNGOS MMAP files. The root path directly contains `001.mmap`/`001YYXX.mmtile`. Missing, partial, off-mesh and liquid routes fail explicitly. Detail-mesh ground following, dynamic obstacles, swimming, jumping, death recovery, taxi flights and cross-map travel graphs are not complete. Ordinary ground navigation has been physically exercised on the Orgrimmar route; this does not prove arbitrary destinations safe. A blocked task can be paused and resumed for a deliberate retry.

No WoW assets or navigation data are distributed. The shared decision source under `native/vendor/policy` is provenance only and is not linked into this executable; the active runtime is the Rust JSON adapter. Keep upstream licenses and notices.

```sh
cargo test --all-targets
```
