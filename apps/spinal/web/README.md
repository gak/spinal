# Spinal browser host

This is a thin browser shell around the same immutable `SourceBundle`,
`ViewerSession`, review clock, and Bevy/Spinal runtime used by the native host.
It is not a second viewer implementation.

With no `data-spinal-manifest` attribute, the page starts in **Open**. Choose one
required Primary directory containing a Spine JSON export, one text atlas, and
its PNG pages. Optionally choose one complete Comparison directory with the same
structure. Spinal bounds and normalizes the selected metadata across both
directories before any Blob read, then reads only the required files and fully
validates every selected immutable bundle as one atomic launch. Primary alone
reveals a paused Preview; Primary plus Comparison reveals a paused Compare. Any
intake failure rejects the whole launch, clears and re-enables both directory
controls, and focuses an actionable error; no partially validated source reaches
the viewer. The selected bytes remain in the tab and are never uploaded; local
host paths and directory roots are not reported.
Open accepts at most 256 selected entries across both directories before any
Blob read. A launch retains at most the existing 128 required runtime files
across both bundles. The per-file and PNG bounds below apply to every file,
while the file-count, aggregate-byte, and aggregate-decoded-texture totals are
one global budget: adding a Comparison never doubles them.

A nonempty `data-spinal-manifest` attribute instead selects the repeatable,
authenticated launch adapter. Version 1 is a strict, immutable same-origin
schema. It pins one required Primary runtime-bundle manifest and one optional
Comparison runtime-bundle manifest by exact byte length and lowercase SHA-256
digest:

```json
{
  "format_version": 1,
  "primary": {
    "url": "current.manifest.json",
    "byte_length": 1234,
    "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
  },
  "comparison": {
    "url": "proposed.manifest.json",
    "byte_length": 1234,
    "sha256": "1111111111111111111111111111111111111111111111111111111111111111"
  }
}
```

Each referenced child is the existing shared `RuntimeBundleManifest`, not a
second browser-only asset format. Its `source` object declares the label, Spine
JSON and atlas virtual paths, and every JSON, atlas, and PNG file with a safe
relative URL, exact length, and digest. Omitting `comparison` launches the same
single-source Preview surface; including it launches the Primary-versus-
Comparison Compare surface. A present but empty launch-manifest attribute is an
explicit configuration error rather than a fallback to local Open.

Those persistent read-only modes are Preview and Compare. Review is reserved
for workflow-only post-build inspection.

Launch-manifest, child-manifest, and asset URLs are safe relative paths resolved
inside their containing manifest’s directory. Every URL must remain on the
page’s exact origin. Requests omit credentials, reject redirects, use no-store
semantics, stream into bounded buffers, and abort after 30 seconds. Duplicate
or escaped URLs, undeclared or unused files, unsafe virtual paths, length or
digest mismatches, wrong schema versions, oversize responses, and incomplete
Spine exports fail closed.

The whole acquisition has one 60-second deadline. The per-request timeout is
the smaller of 30 seconds and the time remaining in that launch, so a long
sequence of stalled files cannot extend the loading state indefinitely.

The fixed browser budgets are deliberately conservative for a 32-bit WebAssembly
process. File-count, downloaded-byte, and decoded-texture totals are one global
budget across Primary and Comparison; adding a Comparison never doubles them:

- 64 KiB per manifest and 128 files across both runtime bundles.
- 16 MiB JSON, 2 MiB atlas, and 16 MiB per non-interlaced 8-bit RGBA PNG.
- 64 MiB total downloaded asset bytes across both runtime bundles.
- PNG dimensions no larger than 4096 by 4096.
- 64 MiB decoded RGBA bytes per page and 192 MiB across every page in both
  runtime bundles.

PNG parsing accepts only the core `IHDR`, `IDAT`, and `IEND` chunks plus fixed-
size `cHRM`, `gAMA`, `sBIT`, `sRGB`, `bKGD`, `pHYs`, and `tIME` metadata in
their valid positions. Text, compressed profile metadata, APNG extensions,
unknown chunks, trailing bytes, and malformed chunk order are rejected before
the image decoder runs. This keeps ancillary decompression outside the viewer's
memory boundary.

Change these only with representative-asset memory evidence. A source that
needs larger limits is blocked explicitly; the viewer does not silently risk
browser memory exhaustion.

## Runtime contract

The browser build uses Bevy’s WebGL2 backend. It loads the same immutable
`SourceBundle`, shared runtime, paused transport, sampled-bounds camera fitting,
and render-layer isolation as the native host. The visible status has four
meanings:

- `Loading`: downloading, linking, or waiting for shared runtime state to
  stabilize.
- `Ready`: drawable output with the supported runtime profile.
- `Ready with warnings`: drawable output using an explicit runtime fallback.
- `Blocked`: download, parse, runtime, graphics, or no-draw failure.

The shell updates its visible text, `aria-busy`, and canvas summary together.
Script, WebAssembly, startup-timeout, panic, and WebGL-context-loss failures are
visible rather than leaving a permanent loading state.

The browser host loads one Primary bundle and, when declared, one Comparison
bundle. It begins paused. The single canvas presents Primary on the left and
Comparison on the right with noninteractive semantic labels. Both views use one
shared animation selection and clock. The semantic HTML controls provide
animation selection, loop mode, fixed playback speeds, absolute timeline
scrubbing, one synchronized skin selection, play or pause, previous frame,
next frame, restart, zoom, and **Fit view**. The skin selector always begins
with the synthetic `Default` choice, followed by the runtime's Primary-order
union and then any Comparison-only skins. It remains usable for a ready
skeleton with no animations. When one pane lacks the selected named skin, that
pane explicitly reports its `Default`-skin fallback instead of pretending the
views match.
Controls stay disabled until the shared runtime snapshot reports them usable.
Labels, animation and skin selection, time, loop state, and speed are reflected
from that same snapshot; JavaScript does not own viewer state. Coordinator
actions remain outside this bridge for now.

Native exposes the same command set through Bevy UI and AccessKit. Both hosts
project the same authoritative runtime snapshot; neither owns a second clock or
transport model.

Preview and Compare use one bounded camera state. Compare fits the union of
Primary and Comparison visible geometry against the conservative shared pane
size, then applies the exact same base mapping and pan/zoom adjustment to both
views. Drag and one-finger touch pan; wheel and two-finger pinch zoom around
their pointer or gesture anchor. When the canvas is focused, arrows pan,
plus/minus zoom, and `F` fits. Browser find and page-zoom modifier shortcuts
remain available. **Fit view** re-samples the selected time and skin, clears
manual navigation, and provides a reliable recovery action. The canvas and
native AccessKit viewport expose the non-live linked/zoom/pan summary without
announcing animation frames.

The contextual Diagnostics disclosure is populated once from the same bounded,
immutable `SourceInspection` used by native Preview and `spinal check`. It
shows source-labelled compatibility, inventory, virtual bundle identity, and
at most eight stable-name findings, with every omission stated explicitly.
Warnings and degradations open the disclosure automatically. Static findings
are not a live region and add no viewer commands or workflow state; authored
text is inserted with DOM `textContent`, never interpreted as markup.

Each launch generates a fresh 256-bit capability with the browser's secure
randomness source. Controls send a size-bounded version 1 string envelope back
to the same window. Rust accepts only exact self-source and same-origin events,
the launch capability, an increasing sequence, fixed actions, and their exact
typed payloads. The additive version 1 `select-skin` action accepts exactly
`{"selection":{"kind":"default"}}` or
`{"selection":{"kind":"named","name":"…"}}`; named values must be nonempty.
Unknown fields or actions, mismatched or extra payloads,
duplicate fields, unsupported versions or speeds, stale sequences, non-string
messages, and malformed envelopes are rejected. The bounded 32-command FIFO
drops the newest command on overflow and shows an explicit warning. Keep this
as a transport into the existing shared `ViewerCommand` inbox; do not add a
parallel JavaScript playback model.

## Internal Phase 0B browser capture

The non-default `phase0b-rehearsal` feature adds a closed generic Bevy 0.19
capture seam around the ordinary browser viewer. It is internal self-authored
plumbing and does not change the production `web` host contract.

A shell runner begins every capture with a fresh 256-bit nonce retained outside
the Node.js CDP driver. The browser acknowledges that nonce with the exact
Current and Proposed manifest/content identities. After both sources are ready,
it creates hidden event-only instances from the exact loaded asset handles,
captures a fresh no-seek `sway`/Once event window through the inclusive
endpoint, and removes
them before executing four fixed samples in sample-major, Current-first order.
For each of the eight presentations, only the selected source camera is active
over the exact 640-by-480 viewport. The browser freezes the accepted semantic
and command generations, validates the isolated presentation across two strict
Bevy updates, and only then requests a screenshot. The driver adds a
two-`requestAnimationFrame` compositor barrier, uses
`Page.captureScreenshot`, and retains the original encoded PNG bytes without
cropping or re-encoding.

Each original screenshot must be a complete static non-interlaced 640-by-480
RGB8 or RGBA8 PNG. Its exact length and SHA-256 become a receipt. The outer
version 3 document requires the strict Current and Proposed event windows and
binds all eight receipts to the corresponding semantic-frame, acknowledged
play, and acknowledged seek generations and exact runtime identities. The Rust
host parser requires the caller to supply the independently retained nonce and
loaded bundle pair; it does not trust the embedded nonce or identities as their
own authority. RGB8 and RGBA8 pixels are normalized to RGBA only in memory for
later comparison, while the captured files remain unchanged.

The driver writes the screenshots, terminal document, and unchanged capture
manifest before it writes a final create-only provenance receipt. That strict
gate-ineligible receipt binds the runner nonce and runtime identities to the
exact manifest and terminal bytes, inventories every served build file, and
records local build/toolchain context, CDP Browser/SystemInfo context, and the
effective page WebGL context. The smoke rechecks those relationships and
owner-only output permissions independently of the driver.

This seam is a `non_representative_rehearsal`. Its capture manifest records
`gate_eligible = false`, and its Rust results are categorically gate-ineligible.
The generated fixture and screenshots are not an independent oracle, Phase 0B
evidence, or a PASS. Its provenance is self-reported/context-only, not process,
browser-executable, or toolchain-distribution attestation.

## Build and hosting

From the repository root, `just web` prepares the self-authored smoke fixture
and runs the manifest-free **Open** page on a foreground-only development server
at `http://127.0.0.1:8424/`. Use `just web 9000` to choose another port.
Choose `apps/spinal/web/bundle/open-primary` for the required Primary directory
and, to exercise Compare, `apps/spinal/web/bundle/open-comparison` for the
optional Comparison directory.
Nothing is installed as a login item or persistent daemon; stopping the command
stops the server.

The `just web`, `just web-build`, and `just web-smoke` recipes are supported on
macOS and Linux, including Linux under WSL. Native Windows can compile and test
the Rust viewer directly, but should run these Unix-shell recipes through WSL
or rely on CI.

`just web-smoke` builds the same fixture, serves it below `/dist/` on
`127.0.0.1:8425`, runs headless Chrome or Chromium, and fails unless the
default local-directory Open Preview/Compare flows and explicit authenticated
Preview/Compare launches all work below a relative path. It first proves a
missing required Primary PNG fails before runtime startup, focuses an error that
exposes no host path, and leaves both directory controls cleared and
ready for an atomic retry.
A corrected Primary-only selection then launches a paused Preview. The smoke
then starts a fresh Open with valid Primary and missing-page Comparison
directories. That paired attempt must identify `Comparison directory:`, clear
and re-enable both controls, reveal no capability or partial viewer, and leak no
host path. Retrying with `bundle/open-primary` and `bundle/open-comparison`
launches a paused Compare and proves the Primary-red and Comparison-blue
attachments render in their exact halves without cross-pane contamination. The
Open and manifest launches report the right mode, Ready state, generic
Primary/Comparison identity, and source-labelled Diagnostics content. The smoke
also drives Zoom In and focused keyboard pan through the browser bridge,
observes both runtime cameras remain linked, and proves **Fit view** returns to
the unmoved 100% state. It requires Bash, Python 3, `curl`, ImageMagick, and
Chrome or Chromium; set `CHROME_BIN` when the browser is not discoverable.

The separate generic Phase 0B capture smoke runs on port 8427 with:

```sh
just phase0b-browser-smoke 8427
```

It prepares the self-authored Current/Proposed fixture, builds the opt-in
`phase0b-rehearsal` WASM mode, serves it only on loopback, and drives the
runner-owned fresh-nonce event-window plus eight-PNG capture through real
headless Chrome or Chromium. It validates the final browser/build/effective-GPU
context receipt, including its exact capture links and served-file inventory.
The complete command passes locally at this Bevy 0.19 implementation checkpoint;
configured CI results for the revision are not claimed. It requires Bash,
Cargo, Trunk 0.21.14, Node.js, `curl`, Python 3, and Chrome/Chromium, but neither
FFmpeg nor ImageMagick. This smoke remains separate from production `web`
build, Clippy, smoke, and MSRV coverage. It remains a gate-ineligible generic
rehearsal: no representative private case/policy, independent oracle,
identity-bound owner runner, publisher, or verifier is supplied by this seam.
Representative Phase 0B remains **NOT RUN**, and mutation remains locked.

`just web-build` writes a relative-URL release build to `web/release-dist`, so
it also works below a path such as `/review/`. Development serving keeps its
watched output in `web/dist`; separating the destinations makes a release build
safe while `just web` is running. Serve `.wasm` as `application/wasm`,
JavaScript as a JavaScript MIME type, JSON as `application/json`, and PNGs as
`image/png`. Hashed JavaScript and WebAssembly may be cached immutably; publish
a manifest and its digest-pinned bundle as one version.

Install the `wasm32-unknown-unknown` Rust target, Trunk 0.21.14, and Just before
using these commands. Release builds use the repository's size-oriented
`web-release` Cargo profile, wasm-bindgen 0.2.126, and Binaryen wasm-opt
`version_123`; these versions are part of the reproducible browser build
contract.

Production hosting should set a CSP that keeps `default-src`, `connect-src`,
and asset loads on `'self'`, disables `object-src`, constrains `base-uri`, and
chooses `frame-ancestors` intentionally for embedded review. Permit
`'wasm-unsafe-eval'` for WebAssembly and use nonces or hashes for Trunk’s inline
bootstrap and the current inline styles. Trunk's development server does not
establish that production policy; verify the deployed response headers rather
than relying on the local host.
