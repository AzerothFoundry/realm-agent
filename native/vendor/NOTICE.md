# Vendored notices

- `detour/` is sourced from the Recast Navigation Detour implementation used by the vMaNGOS build. Retain its upstream license headers in every vendored file.
- `policy/` is a value-only extraction of the AzerothFoundry vMaNGOS decision foundation and remains GPL-2.0-or-later. It is not currently linked into the Rust executable; the Rust strategy adapter is intentionally independent until the shared representation is stabilized.
- No WoW client assets, MPQ files, maps, VMAPs or MMAPs are included.
