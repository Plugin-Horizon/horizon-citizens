# What Horizon lacks for this mod

Against Horizon `a589a3dd9` (hook API 411). Each line: the hook or helper that would serve, what
it is for, and what the mod does today.

- [ ] `Host::set_entity_rotation(dimension: &str, id: u32, yaw: f32, pitch: f32, head_yaw: Option<f32>) -> bool`
  (set `Mob.yaw/body_yaw/head_yaw/head_pitch`, `needs_sync`) — needed for look-close and
  `face`/`look_at` — today: the console command `execute in <dim> run rotate <uuid> <yaw> <pitch>`
  for every change of 2 degrees or more, which turns head and body together (Citizens turns the
  head first).
- [ ] `Host::run_command_as(player: u128, line: &str, op: bool) -> bool` (a command source of
  that player at their own permission level, Bukkit's `Player.performCommand`) — needed for
  `commandtrait` entries with `player: true` — today: `execute as <uuid> at <uuid> run <line>`
  from the console, i.e. with level 4 rights (configurable off).
- [x] Permission nodes: Horizon has `Host::has_permission(player, node, default_level) -> bool`
  (`native_has_permission`, a permissions mod answers through `native_permission_check`). Used now:
  `citizens.*` nodes per command (`commands::NODES`), grants on `Sender`, and `NativeCommand::permission`
  on each registered usage. The bundle needs `"hooks": @API:permission_check@` and
  `@API:command_permission@`; check the bundle declares them (see STATUS.md).
- [ ] `horizon_client::mod_api::parse_snbt(text: &str) -> Result<Compound, String>` (re-export
  of `horizon_world::nbt::parse_snbt`) — needed to call `Host::summon` (which returns the
  entity id) with the data the mod writes as SNBT — today: `run_command("summon ...")` and
  matching the entity in `entity_join_level` by its UUID.
- [ ] `native_living_death(dimension: &str, entity: u32, kind: &str, damage_type: &str, causing: i32)`
  (vanilla `LivingEntity.die`, NeoForge `LivingDeathEvent`) for every living entity — needed to
  tell an NPC's death from its chunk unloading (a guard mod respawns dead NPCs) — today: an NPC
  that took damage within 2 ticks before `entity_leave_level` counts as dead.
- [ ] `entity_join_level` / `entity_leave_level` for displays (`text_display` and the other
  entities Horizon keeps without simulating; today "Not told") — needed to see hologram lines
  that come back with their chunk — today: every hologram summon is preceded by `kill <uuid>`.
- [ ] `Host::navigate_mob(dimension: &str, id: u32, target: [f64; 3], speed: f64) -> bool` and
  `Host::stop_navigation(dimension, id)` (the engine's pathfinding for a mob by id, from any
  server hook; today only from a `Goal` with its `Cx`) — needed for waypoints and a guard
  chasing its target — today: straight-line `/tp` steps every tick, through walls.
- [ ] A `hand` field on `InteractEntityEvent` (main or off hand) — needed to fire one click event
  per click and to match `commandtrait` hands exactly — today: the main hand's call is claimed,
  so the off hand is not asked.
- [ ] `Host::force_chunk` / `forced_chunks` in the catalog's helper list (the `native_force_chunk`
  helper exists, but not on `Host`) — needed for Citizens' `chunks.always-keep-loaded` — today:
  not done (off in the configs this was written for).
- [ ] Mannequins as NPCs: a way to make a `minecraft:player` entity with no connection (a "fake
  player" in the Tab list and seen as a player by other mods) — needed only where other code
  must see NPCs as players — today: mannequins, which look the same to clients but are not
  players to the server.
- [ ] A YAML crate in `Cargo.lock` (`serde_yaml`, `saphyr` or `yaml-rust2`) — needed to read and
  write `saves.yml` and other Bukkit data files — today: the core has its own reader/writer for
  the subset Bukkit writes (`api/core/src/yaml.rs`).
- [ ] Bundles: the launcher stages only the `[api]` folder (`native-api/<id>`), so a path dependency
  from it to a sibling folder of the bundle breaks; documenting that (or letting `[api]` reach a
  `[[crate]]`) would help — needed to keep the testable logic outside the game crate — today: the
  core lives inside the `[api]` folder (`api/core/`, a nested path dependency).

## Public-docs comparison (Citizens2 command and permission lists)

The official wiki answers automated fetches with HTTP 403, so the checklist below comes from a
third-party mirror of its Permissions page and from community command references. Items marked
"not started" have no code here and are not faked.

- [x] `/npc owner [uuid|SERVER]` (`citizens.npc.owner`): shows or sets the saved owner. Player names
  are not resolved (no name lookup in the command path), only the sender's own name; ownership is
  enforced as Citizens does (owner, console or `citizens.admin`).
- [ ] `/npc character`: no node in Citizens' plugin.yml for it under this name. Not started.
- [ ] `/npc power` and `/npc age` (`citizens.npc.power`, `citizens.npc.age`): entity power and age
  settings of the mob or mannequin. Not started; would need an entity-data hook to be real.
- [x] `/npc equip <slot> [item] [amount]` (`citizens.npc.edit.equip`): no GUI; sets or empties a
  slot and respawns the NPC. Item names are not checked against the registry.
- [x] `/npc path add|remove|clear|list` (`citizens.npc.edit.path`): linear waypoints only, no
  path recording and no pathfinding. `/npc text add|remove|clear|list` (`citizens.npc.edit.text`): the
  lines are saved under `traits.text.texts` (guessed layout, not checked against a real Citizens file);
  the chat-based editor is not done.
- [x] Right-click talk: one of the text lines, picked at random, in the `talk.format` chat format
  (`npc.chat.format.no-targets`). `citizens.npc.talk` is NOT in Citizens' plugin.yml, so talk has no node.
- [x] `citizens.*` permission nodes: checked through the hook (see above). `/npc remove` asks
  `citizens.npc.remove` or `citizens.admin` (round 3: that is what Citizens' code checks; plugin.yml does not list it).
- [x] `/citizens help` (`citizens.citizens.help`), added; `/citizens` alone prints it too.
- [ ] `/npc power`, `/npc age` (`citizens.npc.power`, `citizens.npc.age`): no entity power/age hook in
  Horizon's catalog yet.

## One permission node per command (Horizon)

- [ ] A `NativeCommand` takes one `permission(node)` for the whole root: the first usage's node (`npc` gives
  `citizens.npc.info`). A player granted only `citizens.npc.create` therefore gets no `/npc` in the tree and
  is refused before the sub-command check runs. Needed: per-usage nodes, or a root node that passes when any
  of a set of nodes is granted. Today: the sub-command check in `run()` decides, but the root hides first.

## Round 3: compared with Citizens' source (NPCCommands, AdminCommands, en.json, plugin.yml)

Licence: Open Software License 3.0 (reading and porting allowed). 107 `@Command`s in `NPCCommands`,
3 in `AdminCommands`, 4 editor commands in `EditorCommands`; 868 message keys in `en.json`.

Ported (command, flags, argument counts, messages, nodes): `copy --name`, `create -u -s -c --type
--at --nameplate`, `deselect|desel`, `despawn`, `glowing --color`, `gravity`, `hologram
add|insert|set|remove|clear|lineheight|margintop|marginbottom`, `id`, `list [page] -a --owner --type`,
`lookclose --range --randomlook --randompitchrange --randomyawrange`, `moveto`, `name|hidename -h`,
bare `/npc`, `owner`, `remove|rem [all|id|name|uuid] --owner --world --eid`, `rename`, `select|sel`,
`setequipment`, `skin [name] -c -t`, `spawn`, `tp -e`, `tphere|tph|move -c -f`, `type`, `vulnerable`,
`command|cmd add|remove|list` with `-l -r -s -p -o -n --cooldown --gcooldown --delay --n --gn
--permissions --cost --expcost`; `/citizens`, `save`, `reload|load` (confirm within 5 s).

Not ported, each needs a trait or hook Horizon does not give (they are refused with "not supported
on Horizon yet" where they are flags of a ported command, and are unknown commands otherwise):

- [ ] The other ~76 `@Command`s (`age`, `powered`, `sheep`, `slimesize` are done since: they show in the
  summon data): `activationrange aggressive ai armorstand attribute bat
  behavior breakblock chunkload collidable configgui controllable debug disguise drops endercrystal
  enderman entitypose eval flyable forcefield gamemode hitbox home horse inventory item
  itemframe jump knockback leashable metadata minecart mirror mount nophysics ocelot packet painting
  passive pathopt pausepathfinding pickupitems playerfilter playerlist pose
  rabbittype respawn rotationsettings scaledmaxhealth shop shopitem showshop sitting
  skinlayers speak spectate swim target targetable trackingrange undo
  useitem velocity wither wolf` and the `/trait`, `/template` (`/tpl`) and `/waypoints`
  (`/wp`) commands. Each is a Citizens trait or editor; the port has the data model only for the
  traits listed in the README.
- [ ] `create -b -p -t --item --trait --model --registry --temporaryduration --template`: baby age,
  packet NPCs, temporary NPCs and named registries have no equivalent. `-t`/`--duration` of
  `hologram add` and `vulnerable -t` (temporary, not saved) likewise: `vulnerable -t` is accepted
  and saved for good, which differs.
- [ ] `lookclose --filter --perplayer --randomswitchtargets --targetnpcs --headonly --linkedbody
  --disablewhennavigating --randomlookdelay -r`, `skin -l -u -e -s --url --file`, `hologram
  viewrange verticalviewrange updaterate bgcolor edit_npc`, `command execute permissions sequential
  cycle random cost expcost itemcost hideerrors errormessage clearerror forgetplayer`, `setequipment -c`,
  `list --tag --filter --registry`, `select --registry`: refused as above.
- [ ] `undo` and the history of create/remove/copy: not kept.
- [ ] Selecting by looking at an NPC (`select` without arguments ray-traces first in Citizens; here
  only the nearest NPC within the range) and `tphere cursor` (needs a block ray cast; answers
  "Please look at a block to teleport to.").
- [ ] Several NPCs with one name: Citizens asks in a chat conversation; here the prompt lists the ids
  and `/npc select <id>` is the way.
- [ ] Click commands: the port still runs `say ...` and `server ...` commands as plain commands;
  Citizens turns `say` into `npc speak` and `server` into a BungeeCord connect (`Util.runCommand`), and
  adds `--id <id>` to `npc ...` commands. Needs `speak` and a proxy-message hook.
- [ ] `command` limits (`cooldown gcooldown delay n gn cost expcost permissions`, `-n`, execution modes):
  saved in Citizens' format, but a command with one is skipped when clicked (see `unenforced`) and
  `command add` says so.

Guesses (code that is not in the cloned repository, with the file and line):

- `commands.rs:1023` longest NPC name 16 for a player, else 256 (`SpigotUtil.getMaxNameLength`, citizens-api).
- `commands.rs:1302` nine lines a page and the page header in `list` (`Paginator`, citizens-api).
- `commands.rs:739` ownership: the owner, the console or `citizens.admin` (`Owner.isOwnedBy`, citizens-api).
- `commands.rs:925` an unknown word after `/npc` answers "Too many arguments." (the bare `/npc` takes none).
- `commands.rs:50` the cross-world refusal of `tphere` has no text in `en.json`; it shows the message key.
- `commands.rs:1027` any `A-Z0-9_` word is a valid entity type (the port has no list of Bukkit's EntityType).
- `commands.rs:80` `skin -c` prints `{0}` unfilled, as Citizens sends the message without its argument.
- `npc.rs` `add_command_spec`: key order of a new command's fields follows Citizens' `NPCCommandPersister`.

Remaining commands, round 3b: what is left is behaviour of a live mob (AI, pathfinding, follow, wander,
mount, sound, pose, rotate, speed, target, pathto, sitting), GUIs (shops, configgui, inventory, drops),
other plugins (disguise, packet NPCs, behavior files) or a runtime of Citizens (templates, anchors,
eval expressions, player filters). With the hooks in docs/horizon-upstream-gaps.md (`mob_navigate`,
`mob_follow`, `set_entity_rotation`, `entity_merge_nbt`) many could be built, but each needs glue in
`api/src` (not unit-testable here) and its own save format read from Citizens' trait classes, so they
were not written blind. A command stored without effect would have been a fake and was not added.

## Round 3c: real behaviour added (follow, pathto, speed, rotate, anchor, pose, tpto, hurt, playsound, sound, wander)

Works (unit-tested in `core`, glue not compiled against Horizon):
- `follow [name|id] [--margin m] [--enable b]`, `speed`, `pathto me|here|x y z`, `wander [xyrange|delay|pathfind]`:
  the NPC really walks, in a straight line at 0.2 x `navigator.speedmodifier` blocks a tick (follow stops
  2 blocks short, or `--margin`; wander picks a point in the x/y box every `delay` ticks). A straight line
  ignores terrain and walls. Horizon's `Host::mob_navigate`/`mob_follow` only drive a mob with AI, and the
  NPC mannequins are `NoAI`/immovable, so real pathfinding needs NPCs that run AI (a change of the spawn
  data and of look-close/protection that cannot be tested here). Guesses: default follow margin 2 blocks
  (`world.rs` `follow_and_wander`), default wander delay 2 to 6 s there too.
- `rotate --body --head --pitch --towards --toentity`, `pose` (`--save --mirror --assume --remove
  --default -a`, a default pose is held from spawn), `anchor` (`--save --assume --remove -a`), `tpto`,
  `hurt` (`damage` command), `playsound`, `sound` (`--ambient --hurt --death -n -s -d`; hurt, death and
  ambient sounds play through `playsound`). Rotation goes through `Host::set_entity_rotation` (fragment
  `npc_entity_rotation`, `lib.rs` `Op::Rotate`) with the console `rotate` as fallback; head and body
  turn together (`--head` alone is not separate). The metadata keys `ambient-sound`, `hurt-sound`,
  `death-sound` are a guess (the key list is in citizens-api).

Still needs something Horizon does not give:
- [ ] `mount` (no passenger/vehicle hook), `target` and `aggressive` (`Host::mob_attack` is for mobs with AI),
  `controllable`, `jump`, `velocity`, `sitting`, `entitypose`: the NPC has no AI or physics to drive.
- [ ] `inventory`, `drops`, `shop`: the menu hooks exist (`Host::open_menu`, `native_menu_clicked`/`closed`),
  but the port has no inventory or death-drop model to show (NPC deaths are a respawn flag, drops are not
  made) and menu glue cannot be tested here.
- [ ] `follow -p` (protect) needs the damage hook to name the attacker; `pathto cursor`, `anchor -c`: a
  block ray cast.
