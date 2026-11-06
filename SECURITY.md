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
one bound dot with an existing room and an explicit non-repeat Enter press. No background component can
turn a draft into a send. Sending can cause your dot to execute the supplied task.

Credentials go only to fixed HTTPS paths at chatgpt.com:443. TLS checks enabled;
redirects/proxies and ALL automatic retries disabled. Only GET dot/message lists
and room-scoped metadata, image upload and message POST are implemented.
Image downloads use a separate anonymous client, with no account headers,
no cookies/proxies/redirects, public-address preflight and DNS pinning. No room creation, token
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

Keyboard safety: a confirmed progressive keyboard handshake AND a CSI-u Enter
source marker are required for Enter submission. Raw CR is never a send event,
including in an enhanced session. Unsupported/unconfirmed terminals disable
sending and retain editing/newlines. Current global modifiers are not applied
to queued input. No global keyboard permission API is requested.


Images are staged only on an explicit paste or local selection. No upload occurs
until an explicit safe submission and all selected image previews have been
flushed successfully. The existing pending marker is created before the FIRST
upload; an upload or send with an uncertain result retains the marker and draft.
Do not automatically retry an upload after an error, even if no message ID exists.

macOS explicit image paste uses the original Grok file-URL/raster AppleScript
probe with a private 0700 staging directory and 0600 file, a two-second helper
deadline and child file-size cap. Normal exit cleans owned staging directories;
no focus/idle clipboard probe runs. Crash cleanup and OS crash dumps cannot be
guaranteed. Tests never read the real clipboard or call real account services.
Encoded bytes/protocol payloads are redacted from Debug output.
