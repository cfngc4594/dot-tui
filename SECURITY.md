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
redirects and ALL automatic retries disabled. Like Grok Build, the API transport
uses reqwest's proxy environment variables by default; `--no-proxy` explicitly
forces a direct connection. HTTP proxies carry HTTPS through CONNECT, followed
by the existing certificate/hostname-verified TLS connection to ChatGPT. Account
headers are not sent in the CONNECT handshake. Proxy operators can still observe
connection destinations/timing; only use a proxy you trust. No system settings,
PAC/WPAD scripts, OS keychains, or Codex proxy configuration are read or changed.
See [proxy behavior and verification](docs/PROXY.md) for scope and limitations.
Only GET dot/message lists
and room-scoped metadata, image upload and message POST are implemented.
Image downloads use a separate anonymous client, with no account headers,
no cookies/proxies/redirects, public-address preflight and DNS pinning. A generic
`application/octet-stream` declaration allows bounded inspection, never filename-based
trust: the original Grok image-byte validator and full bounded decode must succeed.
Specific image MIME declarations must still match the detected format. No room creation, token
refresh, account changes, computer authorization or hidden mutation endpoints.
Credentials, bodies and transcripts are not logged or persisted. Credentials stay
in process memory; there is no guarantee against memory inspection/crash dumps.

Unix descriptor checks reject non-regular files, wrong ownership, group/other
permission bits and final symlinks. Parent symlinks/extended ACLs are not audited.
Windows credential reads validate the opened handle, reject reparse points, and require current-user ownership with grants restricted to that user, SYSTEM, or Administrators. New private files use a protected current-user/SYSTEM DACL. Cross-platform CI uses offline fixtures, not real account credentials.

Every explicit send gets a random UUID used for request_id/idempotency_token.
While it is in flight, duplicate submissions are rejected, but the composer
remains editable. Completion never overwrites newer input. There is no persistent
send lock and no automatic resend. An unknown result must be checked in the
official app before the user explicitly chooses to send again. This is not a
guarantee of server-side exactly-once delivery.

Read/message requests have a 20-second timeout and connections a 10-second
limit; image uploads have a 120-second total timeout. Terminal and bidi controls
are sanitized. Remote text is untrusted and is never executed. Drafts and input
history exist only in memory and can be lost on exit.

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
flushed successfully. Version 0.8.1 keeps consumed input and image bytes in in-memory input history.
No upload or message is automatically retried. A failed upload means the message
POST was not attempted (an unattached upload may still exist); a failed message
POST has an unknown result. Check the official app before explicitly resending.
Legacy `pending-send` files are neither read nor removed by this version.

macOS explicit image paste uses the original Grok file-URL/raster AppleScript
probe with a private 0700 staging directory and 0600 file, a two-second helper
deadline and child file-size cap. Normal exit cleans owned staging directories;
no focus/idle clipboard probe runs. Crash cleanup and OS crash dumps cannot be
guaranteed. Tests never read the real clipboard or call real account services.
Encoded bytes/protocol payloads are redacted from Debug output.

## Send diagnostics (0.8.1)

Private `~/.dot-tui-send-state/diagnostics.jsonl` records fixed stage names,
start/done, elapsed milliseconds, generated request UUID, HTTP status and an
allowlisted error class. It never records message text, image bytes, filenames,
URLs, account IDs, credentials or raw server error bodies. The log stops
appending at 4 MiB; preserve/rename it while all clients are closed to start a
new log. No evidence is automatically deleted.

Sending consumes the input into session history while editing remains available.
Up in an empty composer recalls the previous submission and its image chips;
Enter is a new explicit send. Unknown failures stay visible during background
refresh. Failure or a late success never overwrites newly typed text. Input
history is memory-only and is lost on exit, as in the existing client; this
release does not add a persistent draft system.

The README stable installer URL uses GitHub `releases/latest/download/install.sh`.
Each published installer retains its own release version, so its archive and checksum
are resolved from one immutable release even if latest changes during installation.
Missing releases or checksum mismatches fail before replacing the executable.
