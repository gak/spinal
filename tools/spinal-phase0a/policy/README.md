# Spine 4.3.23 checked-in policy

These files are reviewed inputs to the Phase 0A harness. They are not evidence
that the gate passed.

The initial calibration was captured on 2026-08-08 from:

- executable: `/Applications/Spine.app/Contents/MacOS/Spine`;
- launcher SHA-256:
  `fea6e69af72dc5a3f38195f293b8af3d61b3bf4b3845b1069b6c3603384340f2`;
- launcher: Spine Launcher 4.3.06, macOS Apple Silicon;
- selected editor: Spine 4.3.23 Professional;
- activation output: hidden by `--hide-license`.

The version probe was:

```text
Spine --update 4.3.23 --hide-license --disable-audio --version
```

Its stdout was 228 bytes with SHA-256
`c4141b1cf70b9bc9d95c22bb37e909a1531dbfb1e8835bcb05a5f074e5e3f0ad`;
stderr was empty.

The capability probe was:

```text
Spine --update 4.3.23 --hide-license --disable-audio --advanced
```

Its stdout was 6,289 bytes with SHA-256
`900b0ba44887c17b7d471f90458af8da786dcb476ded0a308d538bd0d30fdbf1`;
stderr was empty. The stable help body after the three-line host header is
checked in as `spine-4.3.23-advanced-help.txt`.

The JSON export preset was exercised against a disposable, self-authored
fixture. It produced pretty JSON containing nonessential fields and emitted
warnings for a missing image directory or attachment image. Those warnings
were observed despite a zero exit code, so the operation profiles require the
exact quiet-success transcript and reject every extra line.

A calibration capture can only propose a policy update. A separate fresh run
must exercise the reviewed policy, safe staging, typed command wrapper, exact
output discovery, normalization, semantic checks, and evidence writer before
any Phase 0A assertion may pass.
