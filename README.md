# horizon-citizens

> **Unofficial Rust port of Citizens for the Horizon server (https://github.com/arismoko/horizon). Not affiliated with or endorsed by the Citizens authors.**

Citizens-style NPCs as a native mod for [Horizon](https://github.com/arismoko/horizon), the
Rust server/client for Minecraft 26.3: player-shaped NPCs with skins that turn to look at nearby
players, can't be hurt, run commands when clicked and tell other mods about those clicks.
They are kept in a `saves.yml` in the format the Citizens plugin uses, so the same NPCs can be
loaded by Citizens on a Paper server and by this mod on Horizon.

Derived from Citizens (https://github.com/CitizensDev/Citizens2, Open Software License 3.0).
Not affiliated with Citizens or its authors.

> **Built for and working only with [arismoko/horizon](https://github.com/arismoko/horizon)** (the Horizon fork that carries the mod API). [horizon-rust/horizon](https://github.com/horizon-rust/horizon) is experimental for mods and not supported: the earlier version for its native hooks / horizon-mixins is frozen on the `horizon-plain` branch (work stopped on 2026-11-12).

## Which Horizon

This is a native trait mod: `horizon-mod.toml`, a `Cargo.toml` that depends on `horizon-mod-api`, `src/lib.rs` with the hooks, and the plugin logic in the `core/` crate (plain Rust, no engine needed for its tests).

- Pin: `[native] horizon = "54090ee36c4e2c7b43b66db864220c577b94b7e0"` in `horizon-mod.toml` (game `26.3`). That is a development build of arismoko/horizon `main` plus mod API additions that are not all merged upstream yet, so a stock `main` may lack some host methods this mod calls until they land.
- It compiled and loaded on that build and was smoke-tested there; it has not seen a long run with real players. Treat it as experimental.
- Feature descriptions below were written for the earlier horizon-rust version; where they name engine calls (`native_*`) or old file paths, read them as descriptions only. The Known gaps list is current.

## What it does

- **NPCs from `saves.yml`**: reads `npc.<id>` with `name`, `uuid`, `traitnames`, `metadata`
  and `traits`, and writes the file back the same way. Keys it doesn't understand stay as they
  are, Sentinel's data included. A file that is read and then written without changes comes out
  byte for byte the same (checked on five real Citizens files of up to 62 NPCs, see
  `core/examples/roundtrip.rs`, which prints no values).
- **Player NPCs** are `minecraft:mannequin`s, the game's own player-shaped entity: they wear a
  skin, are not in the Tab list and need no fake connection. Other types (`SNOW_GOLEM`, ...) are
  that mob with no AI. Each NPC entity gets the NPC's UUID and the tags `citizens_npc` and
  `citizens_npc_<id>`.
- **Skins** (`skintrait`): the signed texture in `textureRaw`/`signature` goes into the
  mannequin's profile. A skin name without a texture (or, with `try-fetch-default-skin`, the
  NPC's own name) is looked up at Mojang on a separate thread and saved the way Citizens saves it
  (`textureRaw`, `signature`, `metadata.cached-skin-uuid`/`-name`).
- **Look-close** (`lookclose`): the NPC turns to the nearest player within its range (its own
  `range`, else the config's 10 blocks), spectators excepted; with random look on it looks
  around within `randomYawRange`/`randomPitchRange` when nobody is near.
- **Protected** (`metadata.protected`, on unless set to false): no damage from anything,
  including players in creative, and no blow lands.
- **Clicks**: a right click or a hit on an NPC is an event for other mods (`RightClick`,
  `LeftClick` with the NPC's id and UUID and the player), and runs the NPC's `commandtrait`
  commands for that hand (`<p>`/`<player>` is the clicker), once per click cooldown. Commands
  marked as the player's run through `execute as <player>`.
- **Name tag** (`metadata.nameplate-visible`: shown, `hover`, hidden), with `&` colour codes.
- **Glow** (`metadata.glowing`) in the `scoreboardtrait` colour, through a scoreboard team per
  colour (`citizens_<colour>`, no collisions).
- **Holograms** (`hologramtrait`): one `text_display` per line over the NPC, with each line's
  margins and billboard / see-through / shadow settings and `lineHeight` (config default 0.4).
- **Equipment** (`equipment`): the item type and count in each slot. The item's Java-serialised
  `meta` (enchantments, names) can't be read outside Java and is kept in the file but not shown.
- **Gravity** (`gravity.enabled: true` is "does not fall"), **spawned** (`spawned: false` NPCs
  stay out of the world), **owner**.
- **Linear waypoints** (`waypoints.linear.points`): the NPC walks from point to point in a
  straight line at `navigator.speedmodifier` x 0.2 blocks per tick (no pathfinding).
- NPCs appear when a player comes within `spawn-radius` (their chunk is loaded then), go with
  their chunk, are taken out of the world before it is saved at a stop (so no chunk keeps a
  copy), and a copy that comes back with a chunk is taken over or removed.
- **Commands** (operator level 2 by default; the same sub-commands, aliases, argument counts,
  messages and `citizens.npc.*` nodes as Citizens): `/npc` (info), `create [name] [-u] [-s] [-c]
  [--type T] [--at x:y:z:world] [--nameplate true|false|hover]`, `remove|rem [all|id|name|uuid]
  [--owner o] [--world w] [--eid u]`, `select|sel [id|name|uuid] [--range r]`, `deselect|desel`,
  `list [page] [-a] [--owner o] [--type T]`, `id`, `tp|teleport [-e]`, `tphere|tph|move [-c] [-f]`,
  `moveto x y z [world]`, `rename`, `skin [name] [-c] [-t uuid data signature]`,
  `lookclose|look [--range r] [--randomlook b] [--randompitchrange a,b] [--randomyawrange a,b]`,
  `vulnerable`, `glowing [--color C]`, `name|hidename [-h]`, `hologram
  add|insert|set|remove|clear|lineheight|margintop|marginbottom`, `command|cmd
  add [-l|-r] [-s] [-p] [-o] [-n] [--cooldown s] [--gcooldown s] [--delay t] [--n n] [--gn n]
  [--permissions p] <cmd>|remove <id|all>`, `spawn [id|name]`, `despawn [id|name]`, `gravity`,
  `type <type>`, `copy [--name n]`, `owner [uuid|SERVER]`, `setequipment <slot> [item]`, `help`;
  `/citizens` (info), `help`, `save`, `reload|load` (asks twice, as Citizens does). Besides these,
  `text`, `equip` and `path` are plain sub-commands here where Citizens opens an editor.
  Each has its `citizens.*` node (checked through Horizon's permission hook; operators of
  `command-level` pass without it). Right-clicking an NPC with a stick selects it. They also work
  on the server console.

Not done: the equipment and NPC editors (GUIs), other traits (follow, poses, talk-close, shops,
...), other waypoint providers, pathfinding, the other ~80 `/npc` commands and `/trait`,
`/template`, `/waypoints` (list in the Known gaps section), the chat-based text editor, path recording, undo.

## For other mods

`mod_citizens::api` (the core crate's `api` module):

- `subscribe(|event| ...)`: `RightClick`, `LeftClick`, `Spawned`, `Despawned` (with the reason:
  command, chunk unload, death, removal, stop, teleport), `Damaged`, `Died`, `Created`,
  `Removed`, `Selected`, `NavigationComplete`. Delivered once a tick, outside the mod's lock.
- `add_damage_filter(|query| Pass | Cancel | Amount(x))`: asked synchronously for each hit on a
  non-protected NPC (a guard that its friends can't hurt).
- `npcs`, `npc`, `by_uuid`, `by_entity`, `entity_of`, `with_trait("sentinel")`, `selected`.
- `create`, `remove`, `spawn` (also after a death), `despawn`, `teleport`, `navigate_to`,
  `cancel_navigation`, `is_navigating`, `face`, `look_at`, `swing`, `set_equipment`,
  `set_name`, `set_protected`, `set_look_close`, `set_glowing`, `set_skin`, `hologram_lines`,
  `set_hologram_lines`, `commands`, `clear_commands`.
- `trait_data(id, "sentinel")` / `set_trait_data`: a trait's YAML, saved with the NPC, so a mod
  can keep its data where its Paper counterpart keeps it; `metadata` / `set_metadata` likewise.

An NPC that dies (not protected) stays out until `spawn(id)` brings it back.

## Config

`config/citizens.toml`, written with the defaults on the first start
([core/citizens.toml](core/citizens.toml)): `saves-file` (relative to the server folder;
default `plugins/Citizens/saves.yml`, where Citizens keeps it), `command-level`,
`spawn-radius`, `[worlds]` (the overworld's world name; `<name>_nether` and `<name>_the_end`
are the others, plus extra names), `[look-close]`, `[hologram]`, `[skins]`, `[commands]`,
`[selection]`, `[messages]`. Keys follow Citizens' `config.yml` where it has one.

## Build

Build it as a native trait mod with arismoko's launcher (see the "native mods" how-to in the arismoko/horizon docs):

```sh
horizon-launcher native-build <path to this folder> --server-output <dir> --version <tag> --trust-native-code
```

`--trust-native-code` is your consent to compile and run this code inside the server. The logic crate tests run without the engine: `(cd core && cargo test)`.

## Known gaps

What does not work yet on arismoko/horizon (each is marked `TODO(arismoko-api: ...)` in `src/lib.rs`):

- The server folder is taken from the working directory (dedicated servers only).
- `/npc`, `/citizens` and `/trait` sub-commands carry their own `citizens.*` nodes; the bare root commands carry none.
- The `team add citizens_<colour>` console commands used for glow colours fail on arismoko's console (logged, glow colour falls back).

## Licence

Derived from Citizens (https://github.com/CitizensDev/Citizens2, Open Software License 3.0).
This port is under the same licence, see [LICENSE](LICENSE) and [NOTICE](NOTICE).

The first version was written without opening Citizens' source. Since then the commands
(`NPCCommands`, `AdminCommands`), their message texts (`en.json`), permission nodes
(`plugin.yml`) and the traits `/npc` edits were compared with the source and ported line by line;
the source files in question are marked as derived from Citizens. Parts of Citizens' API
(command framework, `Paginator`, `SpigotUtil`) are not in that repository, so their behaviour
here is a guess where the Known gaps section says so. The tests use made-up NPCs.
