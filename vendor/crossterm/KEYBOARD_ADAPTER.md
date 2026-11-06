Based on the official crates.io crossterm 0.28.1 package already in the scoped
Cargo cache (Cargo.lock previously pinned that exact version).

Modified files:
- src/event.rs: add CSI_U_ENTER provenance bit in KeyEventState.
- src/event/sys/unix/parse.rs: set that bit only in CSI-u Enter parsing; raw CR
  and modifyOtherKeys remain unmarked. Corrects decimal multi-digit flags decoding
  (upstream 0.28 treated the first ASCII digit as bits). Adds parser regressions.
- src/terminal/sys/unix.rs: bounded 250ms actual flags query using the existing
  KeyboardEnhancementFlagsFilter/PrimaryDeviceAttributesFilter. Requires raw mode;
  does not discard queued key events or wait indefinitely for a second DA reply.
- src/terminal/sys.rs and src/terminal.rs: expose that query on Unix.

The application's session does query/push/query, validates all requested flags,
and pops exactly its own pushed flags on drop. Protocol confirmation alone is
not sufficient: only a marked CSI-u Enter can reach the send path. Legacy CR
always becomes a newline. No global modifier polling is part of this mechanism.
Original MIT license and Cargo.toml.orig retained. Other source files unchanged.
