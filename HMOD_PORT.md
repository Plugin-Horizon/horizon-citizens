# hmod port: citizens
Round 1
- Fixed:
  - `src/lib.rs` `entity_interact`: host is `HostSide<'_>` (Server arm), returns `Option<InteractionResult>`; a claimed or refused click is `Consume` (was `Flow::Cancel`), fall-through is `None`.
  - `src/lib.rs` permissions: `has_permission` / `permission_value` replaced by `permission_bool` on typed Boolean nodes via new helpers `perm_node`, `holds`, `may_any`; `granted_nodes`, `player_command`, `player_commands_sent` take `&mut dyn ServerHost`; `with_node(&perm_node(..))` for registered sub-commands.
  - `src/lib.rs` `profile_by_uuid` / `profile_by_name`: `host.player(uuid)` and `host.players().player_by_name(name)`.
  - `src/lib.rs`: added `permission_gather_nodes`.
- Permission nodes: every non-wildcard entry of `core::commands::NODES` (about 50 `citizens.*` nodes) -> `NodeDefault::Level(2)` (shipped `command-level`), declared in `permission_gather_nodes` in `src/lib.rs`.
- Gaps:
  - HMOD-GAP(wildcard_nodes): `citizens.*`, `citizens.admin.*`, `citizens.npc.*` and the other wildcard parents cannot be declared as typed nodes (Horizon has no wildcards). A child node is granted only by its own node, not by a held parent; parent queries answer as a denial.
  - HMOD-GAP(profile_cache): Horizon main has no profile lookup for offline players. Owner and name lookups resolve online players only; offline owners show as their UUID. The core's own handling is unchanged.
- Unsure:
  - Node defaults use the shipped `command-level` 2. The config value is read after gathering, so a changed `command-level` does not move the node defaults; the command gate itself still uses the config level.
  - A query error (undeclared node, handler failure) is a denial. The old fallback to the operator level for unanswered nodes is gone.
  - `entity_interact`: `Flow::Cancel` mapped to `InteractionResult::Consume` (no client swing). Not verified against Paper's cancel.
  - Warnings in `commands.rs` (unused `mut`, unused assignment) left as they are.
