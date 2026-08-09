# Spinal Viewer Accessibility Acceptance Runbook

This runbook defines the narrow acceptance evidence for Spinal's read-only
Preview, Compare, Diagnostics, and camera workflow on the current Bevy 0.19.0
and AccessKit 0.24.1 checkpoint. It is separate from Phase 0A and Phase 0B.
Passing it neither authorizes mutation nor establishes Spine 4.3.23
conformance.

The earlier `accessibility-81f065e` package contains immutable, checksummed
Bevy 0.18.1 and AccessKit 0.21.1 automated pre-flight artifacts. Its incomplete
`report.json` remains editable only for the required human review and decision.
The checker recognizes that historical profile only at its recorded repository
commit and pre-flight manifest digest, so it cannot be relabelled or carried
forward as 0.19 acceptance. Migration requires a fresh clean-revision
pre-flight and the complete human keyboard and VoiceOver review.

## Decision boundary

`just accessibility-preflight` runs automation only. Its strongest possible
result is **PRE-FLIGHT PASS**. The generated report deliberately remains
`incomplete` until a named human has completed all required browser and native
keyboard and VoiceOver checks.

The overall decision vocabulary is fixed:

- `pass`: every automated and required human row passed, a named reviewer made
  the human decision, and authorized that completed report's immutable digest
  for recording in the plan;
- `fail`: at least one required row failed or has an unresolved defect; and
- `incomplete`: a required row was not run, the reviewer is unnamed, or the
  report has not received a human decision.

Automation must never write `pass` into the overall decision. A successful
pre-flight can coexist only with an `incomplete` overall report.
Even a completed human `pass` does not change the plan's acceptance record
until the read-only report checker validates the package and its exact printed
digest is recorded in the plan.

## Current acceptance profile

The current profile is intentionally narrow:

- the repository's self-authored generic fixture only;
- one clean, committed repository revision on Bevy 0.19.0 and AccessKit 0.24.1;
- native Spinal on macOS through AccessKit and VoiceOver; and
- the local browser host in the recorded Chrome or Chromium build on macOS,
  also reviewed with VoiceOver.

Record the exact macOS, architecture, display scale, Rust, Node.js, Python,
Trunk, ImageMagick, Chrome/Chromium, GPU backend, and VoiceOver versions. A
later platform, browser, or tool profile requires fresh evidence.

Evidence `format_version = 1` names the report and artifact schema, not a Bevy
version. The checker accepts only the identity-bound historical 0.18.1/0.21.1
profile and current 0.19.0/0.24.1 profile, requires each report to match its
checksummed direct dependency tree, and rejects arbitrary or mixed profiles.
The generator creates only the current profile.

## Automated pre-flight

Choose a new, durable, owner-private directory outside the Git checkout. The
directory must not already exist. From a clean checkout, run:

```text
just accessibility-preflight /absolute/private/path/spinal-accessibility-evidence
```

An optional second argument selects the loopback port used by the existing
browser smoke:

```text
just accessibility-preflight /absolute/private/path/spinal-accessibility-evidence 8430
```

The script uses only Bash, Git, the repository's Rust toolchain, Python 3
standard library, Node.js standard APIs, and the dependencies already required
by `just web-smoke`. Its bounded interactive checks drive the local browser
through Chrome DevTools Protocol directly; no npm package or browser automation
framework is installed.
The pre-flight fails before creating evidence when Node.js does not provide the
global `fetch`, `WebSocket`, and `AbortController` APIs required by that
driver, or when required tool-version provenance cannot be read.
Version one also refuses non-macOS hosts because no other platform is in the
current acceptance profile.
It refuses an existing destination, a destination inside the repository, a
dirty worktree, and an invalid port. It leaves failed or interrupted evidence
in place instead of deleting diagnostic output. After the checks, it verifies
that the same commit is still checked out and the worktree is still clean; a
concurrent repository change makes the automated result fail.

The version-one directory contains:

```text
report.json
checksums.sha256
preflight/
  state.txt
  provenance.txt
  browser-semantics.log
  workspace-tests.log
  browser-smoke-with-500px-preflight.log
```

The pre-flight checks:

1. the browser shell's structural label/reference/live-region contract using
   Python's standard HTML parser;
2. the locked full-workspace test suite, including the existing native
   AccessKit, focus, keyboard-routing, non-color state, and paused-start
   contracts; and
3. the real-Chrome Preview/Compare/render/camera smoke, including its 500 by
   900 narrow accessibility pre-flight for live DOM references and names,
   quiet status, paused transport and timeline, focus-indicator styles,
   44-pixel enabled buttons and selects plus the Loop label, core contrast
   pairs, and page-level horizontal overflow.

The 500-CSS-pixel Chrome run is useful narrow-viewport pre-flight evidence. It
is not actual browser zoom or evidence of 200%/400% reflow, a complete
accessibility tree, trusted keyboard focus order, focus visibility over every
possible artwork color, or assistive-technology speech. Those claims remain
manual.

`checksums.sha256` covers the pre-flight artifacts, not the editable
`report.json`. After manual review, complete every required report field. A
named decision authority sets `decision.result` to `pass` and
`decision.report_digest_recording_authorized` to `true` only when every
required row passed and no material defect remains. Then run:

```text
just accessibility-report-check /absolute/private/path/spinal-accessibility-evidence
```

The checker is read-only. It does not create or change the human decision. It
prints a report digest only after validating the JSON contract, every pre-flight
artifact against `checksums.sha256`, the checksummed state outcomes and
provenance against the report, all automated and human pass invariants, the
named competent reviewer and decision authority, the fixed macOS/Chrome v1
profile, and `phase0b_gate_eligible=false`.

Record that exact digest, the tested commit, the reviewer, and the date in this
plan without changing `report.json` afterward. Any report-byte change requires
another successful check and a replacement plan digest. The plan acceptance
record remains `incomplete` until the exact checker-emitted digest is present.
Do not commit the private evidence directory.

## Required human review

Use the same generic Current and Proposed bundles exercised by the browser
smoke. The native Compare surface can be opened with the generated fixture and
explicit atlases:

```text
just preview apps/spinal/web/bundle/viewer.spine.json \
  --atlas apps/spinal/web/bundle/viewer.atlas \
  --compare apps/spinal/web/bundle/proposed.spine.json \
  --compare-atlas apps/spinal/web/bundle/proposed.atlas
```

Run `just web` for the browser Compare surface. Record observations directly
in `report.json`; optional screenshots, recordings, or notes stay beside it in
the external evidence directory and are named from the corresponding check ID.

### Browser keyboard

With VoiceOver initially off and the mouse or trackpad set aside:

- traverse forward and backward through the canvas, selects, range, checkbox,
  transport buttons, camera controls, and Diagnostics disclosure;
- confirm focus is always visible, follows a sensible order, reaches every
  control, and is never trapped or lost when content changes;
- operate each control with its native keyboard interaction;
- confirm canvas arrows pan, plus/minus zoom, and `F` fits only while the canvas
  is focused; and
- confirm browser find and browser page-zoom shortcuts remain available.

### Browser VoiceOver

With VoiceOver enabled:

- navigate the page landmarks, heading, restrained status, canvas summary,
  controls, camera state, source labels, and Diagnostics disclosure;
- confirm the stable canvas name is `Spinal preview viewport.` in Preview and
  `Spinal comparison viewport. Current is left; Proposed is right.` in Compare;
- distinguish Current from Proposed, selected animation and skin, setup-pose
  and Default-skin fallbacks, paused/running state, and compatibility findings;
- operate the controls and confirm their names and states match the visible
  action; and
- confirm playback frames, time, and camera movement do not create an
  announcement stream. Semantic selection or failure changes may be announced
  once.

### Native keyboard

With VoiceOver initially off and the pointer set aside:

- traverse the viewport and every enabled sidebar action in both directions;
- confirm the focus outline is visible over light and dark fixture content;
- confirm focused off-screen controls are revealed and no scroll region traps
  keyboard use;
- operate buttons with Enter and Space without double activation; and
- confirm viewport arrows and plus/minus control the linked camera only while
  the viewport is focused, with **Fit view** restoring the initial state.

### Native VoiceOver

With VoiceOver enabled:

- navigate the AccessKit viewport, source statuses, controls, skin choices,
  animation choices, and source-labelled Diagnostics;
- confirm roles, names, disabled state, selected skin state, camera summary,
  Current/Proposed identity, and fallback wording are understandable;
- confirm Play/Pause exposes the action that will occur, not a stale label; and
- confirm animation frames and clock time are not repeatedly announced.

### Browser zoom and reflow

In the tested browser, repeat the complete keyboard path at actual 200% and
400% browser zoom. At each level:

- surrounding chrome reflows without lost, overlapped, or unreachable content;
- focused controls remain visible and usable;
- page-level horizontal scrolling is not required for the workflow chrome; and
- any two-dimensional canvas behavior remains confined to the visual content
  exception rather than forcing the controls to scroll in two dimensions.

The automated 500-pixel narrow pre-flight is not a substitute for these two
manual rows.

### Native magnification and minimum window

Native Bevy does not currently expose browser-style content zoom. Test the
minimum supported window, the recorded macOS display scale, and macOS
magnification as an accommodation. Record clipping, focus visibility, and
control reachability. Do not describe this as native 200%/400% reflow or WCAG
reflow evidence.

### Motion, contrast, and non-color state

- Enable Reduce Motion before each host starts. Confirm the viewer still starts
  paused, no incidental chrome motion appears, and authored playback begins
  only after an explicit command.
- Inspect ready, warning/degraded, fallback, and blocked wording without relying
  on color. Verify visible text and focus indicators against both light and dark
  fixture content.
- Treat authored animation, flashing, visual difference approval, and motion
  suitability as a separate qualified visual review or agreed accommodation.

## Pass authority

The human tester must be named and competent to operate VoiceOver. The final
maintainer/reviewer may be the same person for this internal checkpoint, but
the report must say so; independent review is preferred. The reviewer may set
the overall result to `pass` only after every required browser and native row
is `pass`, every automated row is `pass`, and no material defect remains.
They must explicitly authorize immutable-digest recording. The plan's
acceptance record changes only after the checker validates that completed human
decision and its exact emitted digest is recorded in the plan.

An automated pre-flight exit status, a screenshot, an AccessibilityNode unit
test, or a semantic HTML audit cannot make that decision.

## Explicit non-claims

Even a scoped pass does not claim:

- WCAG certification or general product conformance;
- Safari, Firefox, Windows, Linux, NVDA, JAWS, switch control, voice control,
  touch assistive technology, or mobile support;
- native 200%/400% reflow or a general native text-scaling mechanism;
- that AccessKit nodes produce correct VoiceOver speech without the recorded
  human run;
- a textual equivalent for visual pose, animation quality, or Current-versus-
  Proposed visual differences;
- accessibility or photosensitivity safety of user-authored animation;
- future coordinator intake, conflict, approval, recovery, or promotion flows;
- representative Spine runtime correctness, Phase 0A/0B success, mutation
  safety, production readiness, or release readiness; or
- behavior for private assets, arbitrary authored names, languages, browsers,
  operating systems, GPUs, or assistive technologies outside the recorded
  profile.

Use no numeric accessibility score. Preserve failures and `not_run` rows as
evidence rather than rounding them into a pass.
