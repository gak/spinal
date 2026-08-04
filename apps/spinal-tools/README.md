# Spinal command-line tools

`spinal-tools` installs the `spinal` command without adding filesystem, PNG,
or CLI dependencies to the renderer-independent `spinal` runtime crate.

## Loafstead demo check

Run from a Spinal checkout:

```console
cargo run -p spinal-tools -- check --profile loafstead-demo /path/to/cat.json
```

Or install the same binary from an immutable Git revision in CI without
publishing a crate release:

```console
cargo install \
  --git https://github.com/gak/spinal \
  --rev <immutable-commit-sha> \
  --locked \
  spinal-tools
spinal check --profile loafstead-demo --format json /path/to/cat.json
```

Prefer checking out the pinned revision in the CI job and running it with
`cargo run -p spinal-tools` when the job already needs the Spinal repository.
Both forms build the command from the same reviewed commit; neither requires a
GitHub or crates.io release.

### Input discovery

`PATH` is optional and defaults to the current directory. It may name:

- one `.json` skeleton export; or
- a directory containing exactly one `.json` file.

Atlas discovery first tries the matching `.atlas` filename, including the
usual `.spine.json` to `.atlas` mapping, then accepts exactly one sibling
`.atlas` file. Use `--atlas PATH` to select one explicitly. Multiple JSON or
atlas candidates are an error, never an implicit choice.

Page names embedded in the atlas must be relative forward-slash paths inside
the atlas directory. Absolute paths, URL syntax, labels, parent
traversal, Windows drive paths, backslashes, and symlink escapes are rejected.
The same validation runs on every host and rejects Windows device names,
alternate-data-stream syntax, forbidden/control characters, trailing dots or
spaces, and case-insensitive page-name collisions.

The `loafstead-demo` command assumes trusted, editor-produced exports. Before
loading, it accepts at most 8 MiB of skeleton JSON, 1 MiB of atlas text, and
65,536 logical atlas lines. It separately allows at most 64 pages, 256 MiB per
PNG file, 8192 pixels on either page axis, 256 MiB decoded bytes for one page,
and 128 MiPixels across declared pages. These private profile bounds reduce
accidental CI resource spikes. They are not a hostile-input sandbox and do not
bound every standalone-runtime allocation; configurable runtime `LoadLimits`
remain a post-demo item.

### Hard profile failures

The command exits `1` when any of these observable requirements fails:

- exact Spine version `4.3.23`;
- valid JSON plus text atlas, with no Spinal `Degraded` diagnostics;
- every page present as a decodable 8-bit RGBA PNG whose dimensions match the
  text atlas;
- straight alpha, RGBA8888, Linear/Linear filtering, clamp/no repeat, scale 1,
  and only runtime-supported packed rotations;
- at least one drawable default/setup-pose region or mesh;
- nonempty, drawable `walk`, `jump`, `eat`, `sit`, `sleep`, `loaf`, and
  `falling` animations at setup, midpoint, and endpoint;
- these nonempty attachment-only cosmetic skins:
  - `item/hat_red_beret`
  - `item/hat_flower_crown`
  - `item/hat_straw_sunhat`
  - `item/collar_red`
  - `item/collar_bell`
  - `item/collar_founder`
  - `item/glasses_round`
  - `item/glasses_heart`
- one additional nonempty `item/glasses_*` skin for the third glasses design.

Every cosmetic must be selected and visibly drawable in setup pose. The
checker equips every hat + collar + glasses combination and requires all three
layers to remain visible in setup plus the start, midpoint, and endpoint of
every required clip. Every attachment visible without cosmetics and every
piece visible with one cosmetic alone must remain visible in the combination,
so variants that overwrite the body or one another fail.
The third glasses name is reported. Until Loafstead maps that stable name
in its catalogue and Spine bridge, the export can pass but carries an explicit
integration warning; Loafstead CI remains responsible for that external code
mapping.

IK, transform constraints, rigid regions, weighted meshes, unweighted meshes,
linked meshes, multiple pages, and quarter-turn atlas packing are supported
but are not individually required. Coats and a cushion skin are not part of
the demo content gate.

### Warnings

Warnings preserve exit `0`. They call attention to:

- clips shorter than Loafstead's 150 ms SmoothStep crossfade;
- visible loop-boundary differences, root motion, and signed-scale facing
  changes;
- more than 180 degrees of cumulative solved world-orientation travel while
  simulating every pair of required base animations through the production
  crossfade at source phases 0%, 25%, 50%, and 75%; transitions into `walk`
  run at 0.5x, 1x, 1.5x, and 3x;
- underdetermined or beyond-reach IK at representative samples;
- optional future layer animations that are not override-compatible;
- ordinary retained Spinal metadata diagnostics; and
- the absence of a machine-readable saved export/packing preset.

The transition sweep is a deterministic risk detector for problems such as a
head taking a full turn during `walk` to `falling`. It complements visual
review; it does not claim to judge animation quality.

### What final files cannot prove

JSON, atlas, and PNG files cannot reliably certify these authoring settings:

- Bleed;
- padding and edge padding;
- Strip whitespace;
- Animation clean up;
- Nonessential data;
- unresolved editor/export warnings; or
- whether textures are intentionally fully coloured.

Every loaded-export report lists those seven items under `unverified`.
Pre-selection command/source errors list none because there is no selected
export to qualify. Keep the shared Spine export preset, texture-packer
`pack.json`, and warning-free export log with production deliveries.

## Output and exit contract

Human output is the default. `--format json` writes one schema-v1 JSON document
to stdout for passes, profile failures, source failures, and command failures.
Profile failures exit `1`; source/command failures exit `2`; output
serialization or write failures exit `3`.

JSON reports use `schema_version: 1` and include:

- profile name/version and a `pass`, `fail`, `source-error`, or
  `command-error` status;
- canonical JSON and atlas source paths for loaded export reports (`source` is
  `null` before a bundle can be selected);
- Spine version;
- error/warning counts;
- required-animation and cosmetic readiness counts;
- asset, mesh, constraint, page, and animation inventory;
- ordered findings with stable kebab-case code, scope, message, and fix; and
- the seven explicitly unverified authoring settings.

Every status retains the same top-level fields. Consumers should key
automation on `schema_version`, `status`, finding `severity`, and finding
`code`. Human messages and fixes may improve without a schema bump.

Exit codes are:

| Code | Meaning |
|---:|---|
| `0` | Profile passed; warnings may be present |
| `1` | Export failed the selected profile |
| `2` | Invalid command, ambiguous/unsafe source, or filesystem source error |
| `3` | Internal serialization or output error |
