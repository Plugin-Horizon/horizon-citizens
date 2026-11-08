//! Citizens-compatible NPCs for Horizon, without the game: the `saves.yml` format, the NPC
//! model, the console commands that show NPCs in the world, look-close, clicks and their
//! commands, waypoint walking, `/npc`, and the API other mods use. The game glue (`api/`) feeds
//! it the hooks and runs what it asks for.
pub mod api;
pub mod commands;
pub mod config;
pub mod ids;
pub mod look;
pub mod npc;
pub mod saves;
pub mod skin;
pub mod spawn;
pub mod text;
pub mod world;
pub mod yaml;
