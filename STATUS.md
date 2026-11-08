# Status

Version 0.1.0, written blind on 2026-11-06: the core crate's unit tests pass; the trait-mod glue
(`api/`) has never been compiled against Horizon or run.

## Features of the original, as used by the servers this was written for

| Feature (Citizens) | State | Notes |
|---|---|---|
| `saves.yml` read and write, same format | written | Unknown keys kept; five real files (0 to 62 NPCs, up to 210 kB) round-trip byte for byte |
| Player NPCs | written | `minecraft:mannequin` with the NPC's UUID and tags; never in the Tab list |
| Other entity types (`SNOW_GOLEM`, ...) | written | That mob with `NoAI` |
| Skins: `textureRaw` + `signature` | written | Into the mannequin's profile |
| Skins: by name, `try-fetch-default-skin` | written | Mojang lookup on a thread (`ureq`), saved like Citizens (`cached-skin-uuid`) |
| Look-close (`lookclose`: enabled, range, random look and ranges) | written | Through `/rotate`; `realistic-looking` (line of sight) not done |
| Protected (`metadata.protected`) | written | `Invulnerable` + damage hook cancels everything + blows refused |
| Click events for other mods (CoreStage-style: right and left click, NPC UUID, clicker) | written | `api::subscribe` |
| `commandtrait` | partly | hand, player/op flag, `<p>`; one global cooldown; no per-command cooldown, delay, cost, permission or item requirements; player commands run with console rights |
| Name tag (`nameplate-visible` true/false/hover) | written | |
| Glow + team colour (`glowing`, `scoreboardtrait.color`) | written | Scoreboard team per colour |
| `hologramtrait` | written | `text_display` per line, margins, renderer flags, `lineHeight`; lines jump (not glide) after a walking NPC |
| `equipment` | partly | Type and count; the Java-serialised item `meta` is kept but not shown |
| `gravity` | written | `enabled: true` read as "no gravity" (verify against Paper) |
| `spawned`, `owner` | written | |
| `waypoints` (linear) | partly | Straight-line steps by `/tp`, no pathfinding |
| `/npc` basics (create, remove, select, list, id, info, tp, tphere, moveto, rename, skin, lookclose, vulnerable, glowing, name, hologram, command, spawn, despawn, gravity, type, copy, help) | written | Operator level instead of `citizens.*` permissions |
| `/trait` (add, remove, list, toggle), `create --trait`, `/npc info|debug|gamemode|flyable` | written | `/trait` knows `KNOWN_TRAITS` only (sentinel, lookclose, commandtrait, hologramtrait, waypoints, age, gravity, gamemodetrait); `gamemode` and `flyable` are stored, not applied to the body; `debug -i` unsupported. Real Citizens has no `/npc health` or `/npc sneak`. |
| `/citizens save`, `/citizens reload` | written | |
| Selecting with the selection item (stick) | written | |
| API for a Sentinel port (registry, events, damage filter, move/face/swing/equip, trait data) | written | See README |
| `sentinel` trait data | written (kept) | Sentinel itself is another port |
| `inventory`, `followtrait`, `poses`, `rotationtrait`, `targetable`, `leashedtrait`, `chunktickettrait` | not started | Their data is kept as it is |
| `chunks.always-keep-loaded` | not started | Off on these servers; no `force_chunk` on `Host` (GAPS) |
| NPC editors, templates, shops, text/talk-close, paths | not started | Not used |

Counts: written 18, partly 3, not started 3. Core: 56 unit tests, all passing.

## Test first, once a build is possible

1. **It compiles**: the event structs' field names and types (`InteractEntityEvent.entity/stack`,
   `EntityJoinLevelEvent.dimension/entity/kind/loaded_from_disk`, `LivingIncomingDamageEvent`,
   `PerformCommandEvent.player/command`), the `Host` method names (`run_command`, `remove_mob`,
   `entity`, `entity_uuid`, `entity_tags`, `tell`, `feedback`, `server_players`, `world_dir`),
   and that the `[api]` package may depend on `../core` by path.
2. **A mannequin from `/summon`**: `execute in minecraft:overworld run summon minecraft:mannequin
   x y z {UUID:[I;...],profile:{id:[I;...],name:"...",properties:[{name:"textures",value:"...",signature:"..."}]},CustomName:{"text":...},immovable:1b,hide_description:1b,...}`
   on Horizon: skin shows on a vanilla client, name tag, no "NPC" description, not pushable;
   `entity_join_level` fires with kind `minecraft:mannequin` and the UUID reads back.
3. **Commands naming a UUID** through `run_command`: `tp`, `rotate`, `kill`, `data merge entity`,
   `item replace entity`, `team add/modify/join`, `swing`, all behind `execute in <dim> run`.
4. **Clicks**: right click on a mannequin reaches `interact_entity` once (claimed, no off-hand
   call); a hit reaches `attack_entity` and is refused for a protected NPC; a creative player's
   hit does no damage; events reach a listener.
5. **Look-close** looks right (head and body turn together through `/rotate`; how often it is
   sent: every 2 ticks when the angle changed by 2 degrees).
6. **Stop and start**: NPCs are removed before the world is saved (no mannequin in the region
   files), `saves.yml` is written once, the next start spawns them when a player comes near; a
   mannequin that was saved anyway is taken over (or cancelled when already spawned).
7. A real `saves.yml` read and written unchanged comes out byte for byte the same
   (done offline: `cargo run --example roundtrip -- <files>`), and Citizens on Paper
   still loads a file this mod wrote after `/npc create`, `hologram`, `command add`.
8. Holograms: one `text_display` per line at the right height, gone with the NPC, no doubles
   after a chunk reload.

## Second pass: real plugin.yml and config (Citizens 2.0.44)

Read from the original Citizens jar (`plugin.yml` only) and its `config.yml`; no lang file exists in
the jar (its strings are in classes, not read). Wording of messages is therefore NOT checked.

- Permission nodes checked through `has_permission` (operators at `command-level` still pass).
- `/npc owner`, `/npc text` (+ right-click talk), `/npc equip`, `/npc path` basics, `/citizens help`.
- `talk.format` added, default from the real `npc.chat.format.no-targets`. Other defaults in
  `citizens.toml` already match `config.yml` (look-close range 10, random look 3 s = 60 ticks, global
  cooldown 1 s = 20 ticks, hologram line height 0.4, selection item stick, colours).
- Not done: `/npc power`, `/npc age` (no hook), `/npc character`, GUI editors, chat-based text editor.
- Blind: nothing compiled. Check first: `Host::has_permission` exists with `(u128, &str, u8) -> bool`,
  the bundle declares `permission_check` and `command_permission` hooks, `Node::Seq` and `Node::text`
  used in `npc.rs`.

## Public-docs check (command and permission lists)

Checked against public Citizens2 documentation available without the wiki (the official wiki
returns HTTP 403 to automated fetches). Covered: create, remove, select, list, lookclose,
tp, tphere, rename, skin, spawn, despawn, help, plus the basic `/citizens save|reload`. Missing
sub-commands are listed in GAPS.md. Wording and config defaults were not verified against
the official docs, so they are unchanged.

## Command usages (in-game completion), round 2

- `USAGES` now has one usage per real sub-command path with typed arguments and literal choices
  (flags such as `--range`, `--randomlook true|false`, colours, equipment slots, `command add -l -p ...`).
  Free-text forms keep a `text` argument last. 103 usages, up from 35 catch-alls. Checked offline against
  the usage rules of Horizon's `usage_nodes` (`native_server.rs`); not compiled.
- `/npc help` prints `HELP` (syntax without `:kind`), not the registered usages.
- `/citizens` alone is a usage now. `/npc hologram set <line>` without text is a usage error, not "No such line.".
- `/npc remove all` checks `citizens.admin.remove.all` (granted nodes pass), as `plugin.yml` names it.
- Not done: aliases (`/npc cmd`) have no completion of their own; flags in an order not listed are
  accepted as free text but not completed.

## Round 3: command and message parity with Citizens' source

Citizens' source was read (OSL-3.0). `commands.rs` was rewritten around a table of the ported
sub-commands (names, aliases, usage text, argument limits, nodes) and Citizens' message texts from
`en.json`, rendered the way Citizens' `Messaging` does (`[[x]]` highlight, `<yellow>`/`<red>`/`<u>`
tags, `<br>`, `''`). Changes a tester sees: `Created X (ID n).`, `You permanently removed ...`,
`too few / too many arguments` with the usage, ownership checks (`You must be the owner ...`),
`/npc remove` by name or UUID, `/npc name -h` (the old `--visibility` is gone, it is not Citizens'),
command hand default `RIGHT` (was both hands), lowest free command id, `npc.id` prints the bare
id, `/citizens reload` asks twice. `README.md` lists what is ported; `GAPS.md` what is not and every
guess. 25 of 107 `NPCCommands` ported (plus 3 of 3 `AdminCommands`); about 90 of the 868 message
keys are used. Still never compiled: run `cargo test` in `api/core/` first.
Aliases and `citizens load` complete in chat; `npc help` is the first usage, so `citizens.npc.help`
is the root node (as in plugin.yml).
