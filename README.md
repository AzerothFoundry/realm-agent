# realm-agent

Headless WoW 1.12.1 client task runner for the AzerothFoundry realm. The initial supported task is a city duel loop for a selected Mage or Warrior character: supply check, Mage conjuring or inn purchase, local navigation, preparation, allowlisted duel, recovery and continuation.

The service exposes a loopback-only authenticated status/command API and bounded local event stream. It separates the protocol adapter from the value-only task and strategy layers so server-side and client-side policies can share definitions without sharing server `Player`/`Map` pointers.

## Current status

Offline FSM, API/SSE/event-log and local navigation tests pass. The repository does not yet contain a live `benilla-protocol` login/session adapter; no test account has been logged into by this executable, and no live duel should be claimed from this version. See [API](docs/API.md) and [PLAN](PLAN.md).

User-provided 1.12.1 game data and MMap files are required. Navigation data is intentionally not included in the repository.
