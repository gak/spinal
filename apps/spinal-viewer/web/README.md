# Spinal browser host

This is a thin browser shell around the same immutable `SourceBundle`,
`ViewerSession`, review clock, and Bevy/Spinal runtime used by the native host.
It is not a second viewer implementation.

The page loads the URL in its `data-spinal-manifest` attribute. Version 1 is a
strict, immutable same-origin schema. Each dependency carries its exact byte
length and lowercase SHA-256 digest:

```json
{
  "format_version": 1,
  "source": {
    "label": "Example export",
    "json": "export/rig.spine.json",
    "atlas": "export/rig.atlas",
    "files": [
      {
        "path": "export/rig.spine.json",
        "url": "rig.spine.json",
        "byte_length": 12345,
        "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
      },
      {
        "path": "export/rig.atlas",
        "url": "rig.atlas",
        "byte_length": 678,
        "sha256": "1111111111111111111111111111111111111111111111111111111111111111"
      },
      {
        "path": "export/rig.png",
        "url": "rig.png",
        "byte_length": 34567,
        "sha256": "2222222222222222222222222222222222222222222222222222222222222222"
      }
    ]
  }
}
```

Virtual `path` values identify files inside the immutable bundle. URL values
are safe relative paths resolved inside the manifest’s directory. Every URL
must remain on the page’s exact origin. Requests omit credentials, reject
redirects, use no-store semantics, stream into bounded buffers, and abort after
30 seconds. Duplicate or escaped URLs, undeclared or unused files, unsafe
virtual paths, length or digest mismatches, wrong schema versions, oversize
responses, and incomplete Spine exports fail closed.

The fixed browser budgets are deliberately conservative for a 32-bit WebAssembly
process:

- 64 KiB manifest and 128 files.
- 16 MiB JSON, 2 MiB atlas, and 16 MiB per non-interlaced 8-bit RGBA PNG.
- 64 MiB total downloaded bundle bytes.
- PNG dimensions no larger than 4096 by 4096.
- 64 MiB decoded RGBA bytes per page and 192 MiB across all pages.

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

The first vertical slice loads one Primary bundle and begins paused. Browser
transport controls and Compare use the shared command/session seam and land in
subsequent slices; do not add a parallel JavaScript playback model.

## Build and hosting

From the repository root, `just web` prepares the self-authored smoke fixture
and runs a foreground-only development server at `http://127.0.0.1:8424/`.
Use `just web 9000` to choose another port. Nothing is installed as a login
item or persistent daemon; stopping the command stops the server.

The `just web`, `just web-build`, and `just web-smoke` recipes are supported on
macOS and Linux, including Linux under WSL. Native Windows can compile and test
the Rust viewer directly, but should run these Unix-shell recipes through WSL
or rely on CI.

`just web-smoke` builds the same fixture, serves it below `/dist/` on
`127.0.0.1:8425`, runs headless Chrome or Chromium, and fails unless relative
hosting works and the fitted blue attachment is present in the captured canvas
pixels. It requires Bash, Python 3, `curl`, ImageMagick, and Chrome or Chromium;
set `CHROME_BIN` when the browser is not discoverable.

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
