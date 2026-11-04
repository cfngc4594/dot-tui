# Security and limitations

This client uses a non-public ChatGPT backend protocol. It can break without
notice; usage may be unsupported or conflict with service terms. Authentication
is not an official scoped public dot API grant. A copied account token may expose
more than this client's limited operations. Never send login files, credentials,
message dumps or raw transport logs to maintainers.

Launching the interactive application starts a real connection with the existing
official Codex login. The repeated startup prompt has been removed at the user’s
request; this document retains the disclosure. No persistent consent grant, new
credential or expanded account access is created. --read-only disables composing.
Real sends require an
existing room, a draft review, and click/Y confirmation per message. No background component can
turn a draft into a send. Sending can cause your dot to execute the supplied task.

Credentials go only to fixed HTTPS paths at chatgpt.com:443. TLS checks enabled;
redirects/proxies and ALL automatic retries disabled. Only GET dot/message lists
and POST to an existing message room are implemented. No room creation, token
refresh, account changes, computer authorization or hidden mutation endpoints.
Credentials, bodies and transcripts are not logged or persisted. Credentials stay
in process memory; there is no guarantee against memory inspection/crash dumps.

Unix descriptor checks reject non-regular files, wrong ownership, group/other
permission bits and final symlinks. Parent symlinks/extended ACLs are not audited.
Windows live auth is refused until ACL handling exists.

Every real send creates an atomic private pending marker BEFORE queueing network
work, with a random UUID also used for request_id/idempotency_token. The marker
survives crashes and contains no message text, token or account identifier. Server
acceptance clears it; HTTP/network/parse failures remain uncertain and locked.
No automatic resend. Quitting cannot retract a submitted request. Manual recovery
requires first checking the official app; see README. This is defensive duplicate
prevention, not a guarantee of server-side exactly-once delivery.

Transport is bounded at 20 seconds / 4 MiB, text at 16,000 chars, loaded history at
288 messages. Terminal controls and bidi controls are sanitized; remote message
content remains untrusted and must never be executed as commands. Requests already
sent may finish even if the process exits. Drafts exist only in memory.

Current coverage: fake transport/state-machine tests and offline TUI verification.
Real Rust sends still require independent validation. Native macOS 0.4 offline
verification does not establish a working real-account connection. Version 0.7
requires production and development-feature build verification.

The fake backend is compiled only for unit tests or the explicit dev-demo feature.
Do not ship a dev-demo build as production. Production never substitutes simulated
messages when a real connection fails.
