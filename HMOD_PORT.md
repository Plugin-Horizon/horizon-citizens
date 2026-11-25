# hmod port: citizens
Parity: before ~40% (76/190) -> after ~75% (143/190) (method: documented /npc modifiers, /template, /waypoints, nodes counted; a setting that is saved but not applied counts as done only for its command/node/tab, features such as GUIs, pathfinding, per-player filters count as not done); blind, uncompiled.
Round 1
- Fixed:
  - `src/lib.rs` `entity_interact`: host is `HostSide<'_>` (Server arm), returns `Option<InteractionResult>`; a claimed or refused click is `Consume` (was `Flow::Cancel`), fall-through is `None`.
  - `src/lib.rs` permissions: `has_permission` / `permission_value` replaced by `permission_bool` on typed Boolean nodes via new helpers `perm_node`, `holds`, `may_any`; `granted_nodes`, `player_command`, `player_commands_sent` take `&mut dyn ServerHost`; `with_node(&perm_node(..))` for registered sub-commands.
  - `src/lib.rs` `profile_by_uuid` / `profile_by_name`: `host.player(uuid)` and `host.players().player_by_name(name)`.
  - `src/lib.rs`: added `permission_gather_nodes`.
- Permission nodes: every non-wildcard entry of `core::commands::NODES` (about 50 `citizens.*` nodes) -> `NodeDefault::Level(2)` (shipped `command-level`), declared in `permission_gather_nodes` in `src/lib.rs`.
- Gaps:
  - Round 2: wildcard parents (`citizens.npc.*`) are now declared nodes (names ending in `.*` are valid), so a held parent grants its children through `parents_of`.
  - Round 2: owner and name lookups use `host.profile_by_uuid/profile_by_name` (user cache) after the online players.
  - Round 2: `commandtrait` limits are enforced (`permissions`, `cooldown`, `globalcooldown`, `n`, `gn` counted, `delay`, `executionmode` RANDOM/SEQUENTIAL/CYCLE, `hideErrorMessages`, the `ignoreerrors` nodes); error texts from `[commands.error-messages]`. Still skipped with a log line: `cost`, `experienceCost`, `itemCost`, `npc` (no economy; run-as-NPC).
  - Round 2 config: `reload-warning-enabled`, `[skins] rotation-update-degrees` (read, no effect yet), `[talk] multiple-targets-format` (read, no bystander chat yet).
- Audit round: node defaults now follow the configured `command-level` (read at gather time, 2 when the config is not loaded yet); the per-type nodes `citizens.npc.create.<type>` are declared and included in the grants `create` asks (`core::commands::create_type_nodes`), so a non-operator holding `citizens.npc.create.zombie` can create zombies. Still open: `/template`, `/waypoints` and about 100 `/npc` modifiers (work, not API), the `ignoreerrors` nodes (no command-trait cost feature to gate), a parent grant does not pass the engine's per-usage node gate (needs a permission handler that expands wildcards itself, unverified).
- Unsure:
  - Node defaults use the shipped `command-level` 2. The config value is read after gathering, so a changed `command-level` does not move the node defaults; the command gate itself still uses the config level.
  - A query error (undeclared node, handler failure) is a denial. The old fallback to the operator level for unanswered nodes is gone.
  - `entity_interact`: `Flow::Cancel` mapped to `InteractionResult::Consume` (no client swing). Not verified against Paper's cancel.
  - Warnings in `commands.rs` (unused `mut`, unused assignment) left as they are.
- Fix round (docs only, no Citizens source): `/template`, `/waypoints` and 19 `/npc` modifiers added (see README "Docs gaps"); nodes `citizens.templates.*`, `citizens.waypoints.*` and the modifier nodes are declared with the other nodes. Roots `tpl` and `wp` are registered as aliases. Still open: bare roots carry no node (engine), offline owner names (no profile API), mob-specific modifiers, action modifiers (`jump`, `mount`, `speak`, `undo`, `item`, `drops`, `inventory`, `home`, ...), `/trait help` node.

Round 3 (docs only, blind): `commands/mobs.rs` adds 66 `/npc` modifier rows from the wiki table (nodes `citizens.npc.<name>` declared, wildcard parents via `parents_of`, usages, help, tab completion), `World::undo_last` (history of create/remove, 100 steps), `live_nbt` for armorstand/phantom/pufferfish/textdisplay/villager, `speak`, `panimate` arm swings. Open: `behavior configgui copier disguise drops eval inventory jump mount shop showshop spectate target warden` (GUI/editor/other plugin/no API), per-player `playerfilter` effect, `bossbar`/`display`/`hitbox`/`home`/`forcefield`/`pathopt` are saved only. Pathfinding: the host has `mob_move_to`/`mob_move_to_path`, but NPC mobs are `NoAI`, so their navigation never ticks; see GAPS_UPSTREAM (citizens). Unit tests in mobs.rs; not compiled.
