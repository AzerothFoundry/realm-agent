# Local navigation bridge

The bridge uses the same vMaNGOS Detour/Recast navigation library and MMap tile format. It only plans complete ground paths and never emits movement packets or teleports. The realm-agent caller owns packet emission and must rate-limit movement to the server protocol. MMap files are user-provided; none are bundled.

The build defines `DT_POLYREF64`, matching the vMaNGOS build. Map data root must contain `mmaps/NNN.mmap` and compatible `NNNYYXX.mmtile` files. Missing tiles or invalid projection fail closed.
