# Spine export presets

`spine-4.3.23-collaboration-nonessential.export.json` is the reusable preset
for editable collaboration exports from Spine 4.3.23 Professional. Load it in
Spine's export dialog, then choose the current project and a new output folder.
It exports pretty JSON with nonessential data retained and packs attachment
images per skeleton as straight-alpha RGBA8888 PNGs with bleed.

The preset was saved while exporting the official Spineboy Professional
example on 2026-08-10. Its original machine-specific input and output paths
were cleared before check-in; every actual export and texture-packing setting
is otherwise unchanged. The original path-bearing preset had SHA-256
`b488d7c9636ec0c92525eddd3e9ad2060bcb0ac2d623f28b562968981167a1be`.

This preset helps JSON reconstruction but does not replace the source `.spine`
project or source artwork. For a final runtime-only export, use the settings in
[`EXPORT_PROFILE.md`](../EXPORT_PROFILE.md), including Nonessential data off.
The Spineboy project and exported artwork are external licensed fixtures and
are not included with this reusable settings file.
