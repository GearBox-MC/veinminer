# Vein Miner

Hold a key to mine a whole ore vein or tree at once, or dig in shapes: 3x3, 5x5, tunnels, staircases.

Hold the vein key (` by default) while breaking a block: every connected block of the same kind breaks with it.

- Shapes: shapeless vein, small tunnel (1x2), 3x3 tunnel, mining tunnel, staircase up, staircase down, 3x3 and 5x5 squares.
  Cycle them with the mouse wheel while holding the key, or Ctrl + `. Every shape only takes blocks of the kind you aimed at.
- Each block is a real survival break: Fortune, Silk Touch and Unbreaking apply per block, the tool wears per block,
  and drops and experience are gathered where you started.
- Each extra block costs hunger (configurable); a vein stops when you are too hungry.
- Ores group by tag (`c:ores/*`, `minecraft:*_ores`), so deepslate and stone variants mine together. Logs fell whole trees.
- A HUD line shows the shape and how many blocks will break.
- Settings in `config/gearbox_veinminer-common.toml`: max blocks (64), leaves, hunger cost, minimum food level, tag groups.
- English and Simplified Chinese.

Inspired by FTB Ultimine; an independent reimplementation for Horizon, sharing no code or assets with it, and not affiliated with FTB.

## 中文简介

连锁挖矿：按住连锁键挖方块，相连的同类方块一起挖掉；也可以按形状挖（3x3、5x5、隧道、台阶）。附魔逐块生效，每块消耗饥饿值。

## Install

Find **Vein Miner** on the Mods page of the Horizon launcher (Horizon's registry), or download `gearbox_veinminer.hmod` from this
repository's [releases](../../releases), put it in your instance's `horizon-mods/` folder and add
`mod.gearbox_veinminer:true` to `horizon-mods.txt`. Each release names the Horizon Nightly build it was made for.

## Build

```sh
cargo install --locked --git https://github.com/horizon-rust/horizon cargo-horizon-mod
cargo horizon-mod build --wasm      # → target/horizon-mod/gearbox_veinminer.hmod
```

`Cargo.toml` pins the Horizon SDK build (`[package.metadata.horizon] sdk`); `cargo horizon-mod update --horizon <build>`
moves it to another Nightly.

## License

© 2026 HerculeWu. Licensed under [CC BY-NC-SA 4.0](LICENSE). See [NOTICE.md](NOTICE.md) for the third-party code the
built mod contains.
