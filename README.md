# horizon-citizens

## Port status

Compared with Citizens 2.0.x (estimate): **75 %** of the original's commands, permissions, config keys, messages and features work today; **78 %** once our open Horizon PRs are merged; **79 %** with the engine changes we are writing; **75 %** with the newest Horizon SDK. Estimated count: ~150/~190 items (round 2: commandtrait limits and modes, error-message config, wildcard nodes, offline profiles; counted from audit/rec100/citizens.md). Status: not yet compiled after the 2026-11-20 round.

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

Not done: the NPC editors (GUIs), other traits (follow, poses, talk-close,
...), pathfinding, the mob-specific `/npc` modifiers (`/npc wolf`, `/npc horse`, ...) and
the actions (`jump`, `mount`, `speak`, `undo`, ...), the chat-based text editor, path recording.

`/template` (`/tpl`: `list`, `apply`, `generate`, and `create --templates`) and `/waypoints` (`/wp`:
`add`, `remove`, `provider`, `disableteleport`) follow the public Citizens wiki. So do `/npc ai`,
`aggressive`, `passive`, `collidable`, `chunkload`, `leashable`, `targetable`, `knockback`, `swim`,
`pickupitems`, `sitting`, `playerlist`, `skinlayers`, `entitypose`, `trackingrange`,
`activationrange`, `respawn`, `scaledmaxhealth` and `metadata`: they save their setting, and only
`ai` changes the spawned entity (the reply says so for the others).

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
- Needs engine (see ENGINE_NEEDS): the synched states with no `SynchedState` variant yet (axolotl -d, cat -l, panda -e, piglin dancing, wolf -i, enderman -a, sniffer, spellcaster, villager -s). Not ported, unused on the rec servers: behavior trees, `eval`.
- Set on the entity with host calls (round 3): `disguise` (`entity_set_disguise`), `spectate` (`player_set_camera_entity`), warden poses, `targetable`, `leashable`, `trackingrange`, `activationrange`, `enderdragon --destroywalls`, and the synched states of allay, fox, panda, polar bear and vex. NPC mobs follow the game's own pathfinding (`mob_set_navigation_ticks` + `mob_move_to`); a host that refuses the switch keeps the straight-line walk.
- Shops (`/npc shop`, `/npc showshop`, `/npc shopitem`, the `shop` trait): stored in `shops.yml` beside `saves.yml` in Citizens' format; the viewer, the settings/contents/page/item/action editors and the storage viewer are chest menus. Where Citizens uses the cursor you click one of your own stacks (display item, item action stacks), and its anvil text input is the player's next chat line (`null` clears). Not available: the `money` action (no economy; a stored one never pays unless its amount is 0), the trader type opens a merchant screen (`player_open_merchant`) whose trades are the item costs; the game's trade rules take the payments, so purchase limits, commands and non-item actions of a trader shop do not run, JavaScript expressions (`js` + backticks) and PlaceholderAPI inside `papi()` (only the player placeholders `<player>`, `<p>`, `%player%`, `<player_uuid>`, `<world>`), item-action component filters for components the port does not map, `NPCShopPurchaseEvent` for other mods, and the shop permission action reaches LuckPerms through `lp user <uuid> permission set|unset` on the console. Quirks of the original kept on purpose: `/npc shopitem <shop> reset_purchases <number>` always answers "page not found", two editor buttons are hidden by later ones in the same slot, `giveItems` can drop later repeats of a shift-click purchase.
- `/npc inventory` and `/npc drops` edit by clicks (take, deposit, drops chance), not by dragging; both traits use the CitizensAPI save layout; the commandtrait `cost` (money) is skipped (no economy); the `npc` flag runs as the console's `execute as` (see ENGINE_NEEDS `npc_run_command_as_entity`).
- The `team add citizens_<colour>` console commands used for glow colours fail on arismoko's console (logged, glow colour falls back).

## Docs gaps

`/template`, `/waypoints` and the modifiers named above were written from the wiki's command table,
its Templates and Waypoints pages and `plugin.yml` only (Citizens' source was not opened for them).
Where those pages say nothing, this port decided, and the decision is in the module docs of
`src/horizon_citizens_core/commands/modifiers.rs` and `templates.rs`:

- reply texts and save keys (`traits.<modifier>.*`, `metadata.<key>`) are this port's own, so
  Citizens may not read them back;
- unset flags read as off, `collidable` as on; `respawn` is saved as a number, its unit is unknown;
- templates: `override` missing means false; the `commands` section is counted, not run;
  `generate` never overwrites; a name in several namespaces needs `namespace:name`;
- waypoints: indices are 0-based, `remove` without arguments removes the last point, only the
  linear provider keeps points, `hpa` (no description on the wiki) is not implemented.

Round 3 (docs only again): about sixty more `/npc` modifiers (`src/horizon_citizens_core/commands/mobs.rs`):
the mob-specific ones (`allay`, `armorstand`, `wolf`, `villager`, `cat`, `fox`, `horse`, `display`, ...),
`home`, `hitbox`, `forcefield`, `pathopt`, `pausepathfinding`, `rotationsettings`, `bossbar`, `playerfilter`,
`potioneffect`, `attribute`, `speak`, `panimate`, `undo`, `useitem`, `item`, `controllable`, ... They check the mob
type and the values, save under `traits.<modifier>.<key>` and complete in chat. Live on the entity: `armorstand`,
`phantom`, `pufferfish`, `textdisplay`, `villager`; `speak` reaches players in range (5 blocks without `--range`),
`panimate` plays the arm swings, `undo` takes back the last create or remove. Every other setting is saved and the
reply says Horizon does not act on it yet. Still answered "not supported": `behavior`, `configgui`, `copier`,
`eval`, `target` (GUIs, editors, other plugins, or no host API). Pathfinding uses the game's navigation where the host allows it: see HMOD_PORT.md.

## Licence

Derived from Citizens (https://github.com/CitizensDev/Citizens2, Open Software License 3.0).
This port is under the same licence, see [LICENSE](LICENSE) and [NOTICE](NOTICE).

The first version was written without opening Citizens' source. Since then the commands
(`NPCCommands`, `AdminCommands`), their message texts (`en.json`), permission nodes
(`plugin.yml`) and the traits `/npc` edits were compared with the source and ported line by line;
the source files in question are marked as derived from Citizens. Parts of Citizens' API
(command framework, `Paginator`, `SpigotUtil`) are not in that repository, so their behaviour
here is a guess where the Known gaps section says so. The tests use made-up NPCs.

Chat editors (round 3): bare `/npc text` and `/npc path` take your next chat lines (`list`, `remove`, `clear`, `exit`; any other line is added as text), `/npc configgui` renames through one chat line. The commandtrait `npc` flag runs the line as the NPC's own entity (`entity_run_command`).
