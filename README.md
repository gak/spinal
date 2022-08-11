# Spinal

Spinal is a [Spine](http://en.esotericsoftware.com/spine-in-depth) crate for Rust.
It is also a [Bevy](https://bevyengine.org/) plugin.

## ⚠ Status ⚠

* This doesn't work at all yet. It is a work in progress. Come back another time :)

## Clean-room implementation

Spinal is a ["Clean-room design"](https://en.wikipedia.org/wiki/Clean_room_design).
It does not copy any of the official runtimes (and derivatives). I have not looked at the sources.
Everything has been written based on documentation and trial and error with the Spine editor.

This laborious effort was done to use a permissive license that aligns with the Rust community.

See my [thread on the matter](http://en.esotericsoftware.com/forum/Licence-for-a-new-runtime-written-from-scratch-17841)
and the [Spine Runtimes License Agreement](http://esotericsoftware.com/spine-runtimes-license) which would have been the
alternative.

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

You must not read any existing Spine runtime code or their derivatives in order to contribute to this project.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.