# Notice

Vein Miner — © 2026 HerculeWu, licensed under Creative Commons Attribution-NonCommercial-ShareAlike 4.0
International ([LICENSE](LICENSE), SPDX `CC-BY-NC-SA-4.0`).

The built mod (`mod.wasm` inside `gearbox_veinminer.hmod`) also contains third-party code under its own licence:

| Component | Licence | Source |
| --- | --- | --- |
| Horizon mod SDK (`horizon-mod-api`, `horizon-mod-vocab`, `horizon-mod-runtime`, …) | LGPL-3.0-or-later ([LICENSE.LGPL-3.0.txt](LICENSE.LGPL-3.0.txt), which builds on [LICENSE.GPL-3.0.txt](LICENSE.GPL-3.0.txt)) | <https://github.com/horizon-rust/horizon>, at the commit `Cargo.toml` pins |
| `log` | MIT OR Apache-2.0 | <https://crates.io/crates/log> |

The SDK is used unmodified. Its source is the pinned commit of the repository above, and this repository's full source
lets you rebuild the mod against a modified copy of it (`cargo horizon-mod build --wasm`), as LGPL-3.0 section 4 asks.

This project is an independent reimplementation inspired by an FTB mod. It contains no code or assets of FTB mods,
Minecraft, Mojang or Microsoft, and is not affiliated with or endorsed by any of them.
