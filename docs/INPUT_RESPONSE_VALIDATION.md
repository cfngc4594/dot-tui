# Input response validation

Offline native Mac release validation only; no account or real send was used.

Compared with published main 625134252dbfb96d83b274c5dbf0f8d5cb224351 plus
development-only delay/rich-history fixtures, waiting-period typing was lost.
A 1.2s fake connection now paints that typing in 0.181ms and preserves it.

At 288 synthetic rich Markdown messages, baseline five single-character samples
were 340.623–418.201ms; final original-textarea samples were 0.239–0.655ms.
The same short continuous Chinese burst was 5624.382ms before and 0.415ms after.
A wrapped paste tail marker was 343.422ms before and 0.865ms after.
A separate 400-character burst with concurrent PTY I/O completed in 67.642ms;
refresh and resize plus input painted in 2.317ms and 1.474ms respectively.
These are local samples, not service/OS latency guarantees. Raw-mode startup
times varied with OS cache/load; they are not treated as input-latency gains.

Default/dev-demo/release root suites: 116 passed each. Original textarea library:
382 passed. Format, check and strict clippy passed. Original 13 textarea source
files are unchanged, with git-blob checks in its vendor manifest.

The two supplied cropped images were viewed. A focused prompt hides its
placeholder; padding and colors follow original PromptStyle defaults/GrokNight.
100x30 and120x40 buffer comparisons against the actual original textarea passed.
The native cursor keeps its terminal-profile shape. The previously refused
Terminal window was not accessed, so same-screen/font visual identity is unverified.

Published license/validation documents are retained. See the accompanying review
bundle for raw timing JSON, scripts, test logs, provenance and patch verification.
