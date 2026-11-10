# Proxy behavior

The authenticated dot API client follows Grok Build's ordinary `reqwest`
client-builder pattern. It no longer unconditionally disables proxy discovery.
There is no new proxy parser, native OS settings reader, or PAC interpreter.

Reference: Grok Build commit `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`:
- [Ordinary HTTP client](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-http/src/lib.rs#L266-L281)
- [Shared sampling client](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-sampler/src/shared_http.rs#L95-L139)

This is a behavioral adaptation, not a port of Grok's endpoints, authentication,
extra CA loading, redirect policy, retries, or explicit web-fetch proxy service.
The existing dot-tui `reqwest 0.12.28` lock and disabled default features remain
unchanged. No native system-proxy dependency was added.

## Usage

For the common local HTTP CONNECT proxy, launch from a terminal:

```sh
HTTPS_PROXY=http://127.0.0.1:7897 dot-tui
```

This applies only to that launch and does not change macOS/Windows network
settings. The port is an example: use the port of your trusted local proxy.
An `http://` proxy URL does not downgrade ChatGPT HTTPS. It establishes a CONNECT
tunnel and then uses the existing certificate-verified TLS connection to
`chatgpt.com:443` inside the tunnel.

For deliberate direct access, ignoring all proxy environment variables:

```sh
dot-tui --no-proxy
```

Use `--read-only` to disable composing. Restart the app after changing its proxy
environment. The client snapshots configuration when it is constructed.

## Supported scope and limits

- For the fixed HTTPS API, reqwest reads `HTTPS_PROXY`/`https_proxy`, falling back
  to `ALL_PROXY`/`all_proxy`. Nonempty uppercase and lowercase values should not
  conflict; the locked dependency gives uppercase precedence. `HTTP_PROXY` alone
  does not route HTTPS requests.
- `NO_PROXY`/`no_proxy` supplies reqwest's comma-separated bypass rules, including
  domains/subdomains and `*`. Matching is delegated to the locked dependency.
- This build does **not** read macOS Network proxies, Windows Internet Options,
  desktop-specific Linux proxy preferences, native exclusions, PAC, or WPAD.
  Setting a system proxy alone is not sufficient. With no applicable proxy
  environment variable, reqwest connects directly, as in the referenced Grok
  source. PAC-only setups must expose a supported proxy endpoint themselves.
- Do not assume a SOCKS URL works: this build does not enable reqwest's `socks`
  feature. Use an HTTP/HTTPS CONNECT endpoint. Malformed values and CGI
  `REQUEST_METHOD` behavior are inherited from reqwest; this change deliberately
  adds no custom fallback or validation semantics.
- Proxy URLs may contain separate proxy credentials, which reqwest handles as
  proxy authentication. Prefer a trusted local proxy that does not need them.
  Do not put account tokens in proxy URLs. HTTP proxy authentication may be
  visible on the connection to that proxy; use a verified HTTPS proxy when
  confidentiality of proxy credentials is required.

## Security boundary

The destination is still fixed to allowed HTTPS ChatGPT API paths. Certificate
and hostname validation, no redirects, and no automatic retries remain in place.
The account Authorization and account-ID headers stay inside the target TLS
connection, never in CONNECT. No endpoint override, CA override, invalid-TLS
switch, credentials logging, or automatic proxy-to-direct retry was added.
Proxy operators can observe destinations and connection timing.

Anonymous image downloads remain separate, direct, and DNS-pinned to public
addresses. They do not inherit API proxy settings or account headers. This
protects the existing SSRF boundary; images can still fail on a network requiring
a proxy even when API reads work.

## Verification

Loopback-only child-process tests cover uppercase/lowercase proxy variables,
HTTPS-over-ALL precedence, NO_PROXY domain/suffix/wildcard matching, explicit
`--no-proxy`, direct access with no applicable variable, CONNECT header isolation,
TLS handshake after CONNECT, rejection of plaintext HTTP, and no retry or direct
fallback after a proxy/TLS failure. They never read real account credentials,
change process-global environment variables, or contact a public service.

Native validation and any live connectivity results are recorded separately in
`PROXY_VALIDATION.md`. Offline tests do not establish real account access,
message delivery, or native Windows/Linux runtime behavior.
