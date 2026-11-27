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
    /// `entity_set_synched_value`.
    Value(Value),
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
    /// `entity_set_no_physics` (`/npc nophysics`).
    NoPhysics(bool),
    /// `entity_set_velocity` (`/npc velocity`), blocks per tick.
    Velocity([f64; 3]),
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
    AxolotlPlayingDead,
    CatLying,
    PandaEating,
    PiglinDancing,
    EndermanCreepy,
    WolfInterested,
}

/// What a sniffer is doing (`Sniffer.State`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sniffer {
    Idling,
    FeelingHappy,
    Scenting,
    Sniffing,
    Searching,
    Digging,
    Rising,
}

impl Sniffer {
    /// The saved word (upper case with `_`, as the `--state` option saves it).
    pub fn parse(word: &str) -> Option<Sniffer> {
        Some(match word.trim().to_ascii_uppercase().as_str() {
            "IDLING" => Sniffer::Idling,
            "FEELING_HAPPY" => Sniffer::FeelingHappy,
            "SCENTING" => Sniffer::Scenting,
            "SNIFFING" => Sniffer::Sniffing,
            "SEARCHING" => Sniffer::Searching,
            "DIGGING" => Sniffer::Digging,
            "RISING" => Sniffer::Rising,
            _ => return None,
        })
    }
}

/// The spell an evoker or illusioner casts (`SpellcasterIllager.IllagerSpell`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spell {
    None,
    SummonVex,
    Fangs,
    Wololo,
    Disappear,
    Blindness,
}

impl Spell {
    pub fn parse(word: &str) -> Option<Spell> {
        Some(match word.trim().to_ascii_uppercase().as_str() {
            "NONE" => Spell::None,
            "SUMMON_VEX" => Spell::SummonVex,
            "FANGS" => Spell::Fangs,
            "WOLOLO" => Spell::Wololo,
            "DISAPPEAR" => Spell::Disappear,
            "BLINDNESS" => Spell::Blindness,
            _ => return None,
        })
    }
}

/// The values `entity_set_synched_value` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Sniffer(Sniffer),
    Spell(Spell),
    /// A villager's unhappy counter in ticks (`-s` is vanilla's `setUnhappy`: 40).
    VillagerUnhappy(i32),
}

/// Ticks of a villager's head shake that `AbstractVillager.setUnhappy` sets.
pub const VILLAGER_SHAKE_TICKS: i32 = 40;

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
    ("minecraft:axolotl", "axolotl", "d", Synched::AxolotlPlayingDead),
    ("minecraft:cat", "cat", "lying", Synched::CatLying),
    ("minecraft:panda", "panda", "eating", Synched::PandaEating),
    ("minecraft:piglin", "piglin", "dancing", Synched::PiglinDancing),
    ("minecraft:enderman", "enderman", "angry", Synched::EndermanCreepy),
    ("minecraft:wolf", "wolf", "interested", Synched::WolfInterested),
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
    if let Some(b) = flag(npc, "nophysics", "enabled") {
        out.push(EntitySet::NoPhysics(b));
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
    // The values with a word or a counter: a sniffer's state, a spellcaster's spell, a villager's head shake (a zombie villager is not an `AbstractVillager`: its `-s` stays saved only).
    let word = |name: &str, key: &str| npc.map.at(&["traits", name, key]).and_then(Node::text);
    if kind == "minecraft:sniffer"
        && let Some(state) = word("sniffer", "state").and_then(|w| Sniffer::parse(&w))
    {
        out.push(EntitySet::Value(Value::Sniffer(state)));
    }
    if matches!(kind, "minecraft:evoker" | "minecraft:illusioner")
        && let Some(spell) = word("spellcaster", "spell").and_then(|w| Spell::parse(&w))
    {
        out.push(EntitySet::Value(Value::Spell(spell)));
    }
    if kind == "minecraft:villager"
        && let Some(b) = flag(npc, "villager", "shake_head")
    {
        out.push(EntitySet::Value(Value::VillagerUnhappy(if b { VILLAGER_SHAKE_TICKS } else { 0 })));
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
    fn the_saved_synched_only_states_and_values_become_host_calls() {
        let mut n = npc("minecraft:cat");
        n.map.put(&["traits", "cat", "lying"], Node::bool(true));
        assert_eq!(settings(&n, "minecraft:cat"), vec![EntitySet::Synched(Synched::CatLying, true)]);
        // A state of another kind is not sent to this one.
        assert!(settings(&n, "minecraft:wolf").is_empty());

        let mut n = npc("minecraft:sniffer");
        n.map.put(&["traits", "sniffer", "state"], Node::str("FEELING_HAPPY"));
        assert_eq!(settings(&n, "minecraft:sniffer"), vec![EntitySet::Value(Value::Sniffer(Sniffer::FeelingHappy))]);
        n.map.put(&["traits", "sniffer", "state"], Node::str("NAPPING"));
        assert!(settings(&n, "minecraft:sniffer").is_empty());

        let mut n = npc("minecraft:evoker");
        n.map.put(&["traits", "spellcaster", "spell"], Node::str("WOLOLO"));
        assert_eq!(settings(&n, "minecraft:evoker"), vec![EntitySet::Value(Value::Spell(Spell::Wololo))]);

        let mut n = npc("minecraft:villager");
        n.map.put(&["traits", "villager", "shake_head"], Node::bool(true));
        assert_eq!(settings(&n, "minecraft:villager"), vec![EntitySet::Value(Value::VillagerUnhappy(40))]);
        n.map.put(&["traits", "villager", "shake_head"], Node::bool(false));
        assert_eq!(settings(&n, "minecraft:villager"), vec![EntitySet::Value(Value::VillagerUnhappy(0))]);
        // A zombie villager is no `AbstractVillager`.
        assert!(settings(&n, "minecraft:zombie_villager").is_empty());
    }

    #[test]
    fn disguise_names_become_ids() {
        assert_eq!(disguise_id("ZOMBIE"), "minecraft:zombie");
        assert_eq!(disguise_id("minecraft:wither_skeleton"), "minecraft:wither_skeleton");
    }
}
