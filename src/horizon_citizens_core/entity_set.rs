//! Settings of an NPC's entity that the summon data cannot carry: they are set through host calls
//! after the entity joined (`Op::Entity`), from what the NPC saved.
//!
//! Save keys are the port's own (see `commands/modifiers.rs`): `traits.<name>.enabled`,
//! `traits.<name>.value`, and the mob modifiers' `traits.<name>.<key>`; the disguise is the original's
//! `traits.disguise.type` (`DisguiseTrait`, `@Persist EntityType type`).

use super::npc::NpcData;
use super::tree::TreeExt;
use super::yaml::Node;

/// One host call for an NPC's entity.
#[derive(Clone, Debug, PartialEq)]
pub enum EntitySet {
    /// `entity_set_disguise`: the entity shown as this type (`minecraft:...`), or as itself.
    Disguise(Option<String>),
    /// `entity_set_pose` (a warden's `dig`, `emerge`, `roar`).
    Pose(Pose),
    /// `entity_set_synched_state`.
    Synched(Synched, bool),
    /// `entity_set_targetable`.
    Targetable(bool),
    /// `entity_set_leashable`.
    Leashable(bool),
    /// `entity_set_tracking_range`, in blocks (`None`: the type's again).
    TrackingRange(Option<f64>),
    /// `entity_set_activation_range`, in blocks (`None`: the setting's again).
    ActivationRange(Option<f64>),
    /// `dragon_set_destroy_blocks`.
    DestroyBlocks(bool),
    /// `mob_set_navigation_ticks`: a `NoAI` mob follows the paths it is given. The glue answers with
    /// [`super::world::World::nav_ready`] when the host took it.
    NavigationTicks(bool),
    /// `mob_move_to`: path to the point; `speed` is the movement speed modifier (1.0 = the mob's own).
    MoveTo { target: [f64; 3], speed: f64 },
    /// `mob_stop`.
    StopNavigation,
}

/// The poses `/npc warden` sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Digging,
    Emerging,
    Roaring,
}

/// The flags `entity_set_synched_state` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Synched {
    FoxInterested,
    FoxPouncing,
    FoxFaceplanted,
    PandaSneezing,
    PandaRolling,
    PandaSitting,
    AllayDancing,
    VexCharging,
    PolarBearStanding,
}

/// (entity kind, trait name, saved key, state): the modifiers whose value only lives in synched data.
const SYNCHED: &[(&str, &str, &str, Synched)] = &[
    ("minecraft:allay", "allay", "dancing", Synched::AllayDancing),
    ("minecraft:fox", "fox", "interested", Synched::FoxInterested),
    ("minecraft:fox", "fox", "pouncing", Synched::FoxPouncing),
    ("minecraft:fox", "fox", "faceplanted", Synched::FoxFaceplanted),
    ("minecraft:panda", "panda", "sneezing", Synched::PandaSneezing),
    ("minecraft:panda", "panda", "rolling", Synched::PandaRolling),
    ("minecraft:panda", "panda", "sitting", Synched::PandaSitting),
    ("minecraft:polar_bear", "polarbear", "rearing", Synched::PolarBearStanding),
    ("minecraft:vex", "vex", "charging", Synched::VexCharging),
];

/// The entity id Bukkit's `EntityType` name stands for (`ZOMBIE` -> `minecraft:zombie`).
pub fn disguise_id(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    if n.contains(':') { n } else { format!("minecraft:{n}") }
}

fn flag(npc: &NpcData, name: &str, key: &str) -> Option<bool> {
    npc.map.at(&["traits", name, key]).and_then(Node::as_bool)
}

fn number(npc: &NpcData, name: &str) -> Option<f64> {
    let n = npc.map.at(&["traits", name, "value"])?;
    n.as_f64().or_else(|| n.text().and_then(|t| t.trim().parse().ok()))
}

/// Everything an NPC saved that is set through host calls, for the entity of `kind` (`minecraft:...`).
/// Only what was set is listed: an NPC that never touched a setting keeps the game's own.
pub fn settings(npc: &NpcData, kind: &str) -> Vec<EntitySet> {
    let mut out = Vec::new();
    if let Some(t) = npc.map.at(&["traits", "disguise", "type"]).and_then(Node::text) {
        out.push(EntitySet::Disguise(Some(disguise_id(&t))));
    }
    if let Some(b) = flag(npc, "targetable", "enabled") {
        out.push(EntitySet::Targetable(b));
    }
    if let Some(b) = flag(npc, "leashable", "enabled") {
        out.push(EntitySet::Leashable(b));
    }
    // Citizens' `-1` takes the setting back.
    if let Some(v) = number(npc, "trackingrange") {
        out.push(EntitySet::TrackingRange((v >= 0.0).then_some(v)));
    }
    if let Some(v) = number(npc, "activationrange") {
        out.push(EntitySet::ActivationRange((v >= 0.0).then_some(v)));
    }
    if kind == "minecraft:ender_dragon"
        && let Some(b) = flag(npc, "enderdragon", "destroywalls")
    {
        out.push(EntitySet::DestroyBlocks(b));
    }
    for (k, name, key, state) in SYNCHED {
        if *k == kind
            && let Some(b) = flag(npc, name, key)
        {
            out.push(EntitySet::Synched(*state, b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npc(kind: &str) -> NpcData {
        NpcData::new(1, 1, "N", kind)
    }

    #[test]
    fn nothing_saved_nothing_set() {
        assert!(settings(&npc("FOX"), "minecraft:fox").is_empty());
    }

    #[test]
    fn disguise_names_become_ids() {
        assert_eq!(disguise_id("ZOMBIE"), "minecraft:zombie");
        assert_eq!(disguise_id("minecraft:wither_skeleton"), "minecraft:wither_skeleton");
    }
}
