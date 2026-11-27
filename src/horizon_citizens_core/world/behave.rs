//! The NPCs' behavior trees at run time (`BehaviorTrait` with its `RootBehavior`), the expression scope an NPC
//! gives (`NPCExpressionScope`), the `/npc eval` and `/npc behavior` support and the glue-read health and motion.
//!
//! The tree itself is `behavior`; this file is what ties it to an NPC of the world: it builds the scope from the
//! NPC, runs the tree once a tick through [`BehEnv`] and applies what expressions did to the inventory.

use super::World;
use crate::horizon_citizens_core::behavior::{Cx, Env, Signals, Status, Tree};
use crate::horizon_citizens_core::expr::{self, Fx, Memory, Scope, Value};
use crate::horizon_citizens_core::npc::{Location, StackData};
use crate::horizon_citizens_core::tree::TreeExt;
use crate::horizon_citizens_core::yaml::Node;
use crate::horizon_citizens_core::{look, text};

/// Health and motion the glue read for an NPC that runs a behavior tree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveStats {
    pub health: f64,
    pub max_health: f64,
    pub velocity: [f64; 3],
}

/// One NPC's tree and what it keeps between ticks.
pub(super) struct BehaviorRun {
    tree: Option<Tree>,
    /// The `traits.behavior.tree` node the tree was parsed from (a change builds a new tree).
    src: Option<Node>,
    scope: Scope,
    running: bool,
    /// The memory as it was last written to the NPC.
    saved: Memory,
}

impl BehaviorRun {
    fn new(memory: Option<&Node>) -> BehaviorRun {
        let mut scope = Scope::new();
        if let Some(m) = memory {
            scope.memory.load_from(m);
        }
        let saved = scope.memory.clone();
        BehaviorRun { tree: None, src: None, scope, running: false, saved }
    }
}

/// The game slot name of an expression's equipment slot, as the NPC's `equipment` keys have it.
fn equipment_key(slot: &str) -> Option<&'static str> {
    Some(match slot {
        "hand" | "mainhand" => "hand",
        "off_hand" | "offhand" => "off_hand",
        "head" | "helmet" => "helmet",
        "chest" | "chestplate" => "chestplate",
        "legs" | "leggings" => "leggings",
        "feet" | "boots" => "boots",
        "body" => "body",
        "saddle" => "saddle",
        _ => return None,
    })
}

struct BehEnv<'a> {
    w: &'a mut World,
    id: i32,
}

impl BehEnv<'_> {
    fn at(&self) -> Option<Location> {
        self.w.current_location(self.id)
    }

    fn speed(&self, modifier: Option<f64>) -> f64 {
        let base = self.w.saves.get(self.id).map_or(1.0, |n| n.speed_modifier());
        0.2 * base * modifier.unwrap_or(1.0)
    }
}

impl Env for BehEnv<'_> {
    fn spawned(&self) -> bool {
        self.w.is_spawned(self.id)
    }

    fn navigating(&self) -> bool {
        self.w.is_navigating(self.id)
    }

    fn walk_to(&mut self, target: [f64; 3], speed: Option<f64>) {
        let s = self.speed(speed);
        self.w.navigate_to(self.id, target, s);
    }

    fn wander(&mut self, radius: i64) -> bool {
        let Some(at) = self.at() else { return false };
        let r = radius.max(1) as f64;
        let (dx, dz) = (self.w.rng.between(-r, r), self.w.rng.between(-r, r));
        let s = self.speed(None);
        self.w.navigate_to(self.id, [at.x + dx, at.y, at.z + dz], s)
    }

    fn look_at(&mut self, p: [f64; 3]) {
        let Some(at) = self.at() else { return };
        let (yaw, pitch) = look::look_at(look::eye([at.x, at.y, at.z]), p);
        self.w.face(self.id, yaw, pitch);
    }

    fn teleport(&mut self, world: Option<&str>, p: [f64; 3]) {
        let Some(at) = self.at() else { return };
        let loc = Location {
            world: world.map_or_else(|| at.world.clone(), str::to_owned),
            x: p[0],
            y: p[1],
            z: p[2],
            yaw: at.yaw,
            pitch: at.pitch,
            body_yaw: at.body_yaw,
        };
        self.w.teleport(self.id, loc);
    }

    fn command(&mut self, line: &str, op: bool, player: bool) {
        let Some(npc) = self.w.saves.get(self.id).cloned() else { return };
        let mut cmd = line.trim_start_matches('/').to_owned();
        if let Some(rest) = cmd.strip_prefix("say") {
            cmd = format!("npc speak \"{}\" --id {}", rest.trim().replace('"', "'"), self.id);
        }
        if (cmd.starts_with("npc ") || cmd.starts_with("waypoints ") || cmd.starts_with("wp ")) && !cmd.contains("--id ") {
            cmd.push_str(" --id <id>");
        }
        let cmd = text::fill_placeholders(&cmd, "", &npc.name(), self.id);
        if player && npc.is_player() {
            if let Some(entity) = self.w.entity_of(self.id) {
                self.w.ops.push(super::Op::EntityCommand { entity, line: cmd, level: if op { 4 } else { 0 } });
                return;
            }
        }
        self.w.ops.push(super::Op::Console(cmd));
    }

    fn say(&mut self, msg: &str) {
        let line = format!("npc speak \"{}\" --id {}", msg.replace('"', "'"), self.id);
        self.w.ops.push(super::Op::Console(line));
    }

    fn log(&mut self, msg: &str) {
        eprintln!("[citizens] behavior of NPC {}: {msg}", self.id);
    }

    fn distance_to(&self, p: [f64; 3]) -> f64 {
        match self.at() {
            Some(a) => look::dist2([a.x, a.y, a.z], p).sqrt(),
            None => f64::MAX,
        }
    }

    fn break_block(&mut self, p: [i64; 3]) {
        let Some(dim) = self.w.dimension_of(self.id) else { return };
        self.w.ops.push(super::Op::Console(format!("execute in {dim} run setblock {} {} {} air destroy", p[0], p[1], p[2])));
    }

    fn find_npc(&self, s: &str) -> Option<i32> {
        let s = s.trim();
        if let Ok(id) = s.parse::<i32>() {
            return self.w.saves.get(id).map(|n| n.id);
        }
        let uuid = crate::horizon_citizens_core::ids::parse_uuid(s)?;
        self.w.saves.npcs.iter().find(|n| n.uuid() == Some(uuid)).map(|n| n.id)
    }

    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
    }

    fn random(&mut self) -> f64 {
        self.w.rng.next_f64()
    }
}

impl World {
    /// The glue read this NPC's health and motion (only asked for NPCs with a behavior tree).
    pub fn set_live_stats(&mut self, id: i32, stats: LiveStats) {
        self.live_stats.insert(id, stats);
    }

    /// The NPCs whose entities the glue reads each tick for their expression scope: (NPC id, entity).
    pub fn behavior_entities(&self) -> Vec<(i32, u32)> {
        self.saves
            .npcs
            .iter()
            .filter(|n| n.has_trait("behavior"))
            .filter_map(|n| self.entity_of(n.id).map(|e| (n.id, e)))
            .collect()
    }

    /// `NPCExpressionScope.createFor`: the NPC's values as the expressions name them.
    pub(super) fn fill_scope(&self, id: i32, sc: &mut Scope) {
        let Some(npc) = self.saves.get(id) else { return };
        sc.vars.clear();
        sc.set("npc.id", Value::Num(f64::from(id)));
        sc.set("npc.name", Value::Str(npc.name()));
        sc.set("npc.uuid", Value::Str(npc.uuid().map(crate::horizon_citizens_core::ids::format_uuid).unwrap_or_default()));
        sc.set("npc.spawned", Value::Bool(self.is_spawned(id)));
        sc.set("npc.protected", Value::Bool(npc.protected()));
        let flyable = npc.map.at(&["traits", "flyable", "flyable"]).and_then(Node::as_bool).unwrap_or(false);
        sc.set("npc.flyable", Value::Bool(flyable));
        let at = self.current_location(id);
        let (x, y, z, yaw, pitch) = at.as_ref().map_or((0.0, 0.0, 0.0, 0.0, 0.0), |l| (l.x, l.y, l.z, f64::from(l.yaw), f64::from(l.pitch)));
        sc.set("npc.x", Value::Num(x));
        sc.set("npc.y", Value::Num(y));
        sc.set("npc.z", Value::Num(z));
        sc.set("npc.yaw", Value::Num(yaw));
        sc.set("npc.pitch", Value::Num(pitch));
        let st = self.live_stats.get(&id).copied().unwrap_or_default();
        sc.set("npc.health", Value::Num(st.health));
        sc.set("npc.maxhealth", Value::Num(st.max_health));
        sc.set("npc.velocity.x", Value::Num(st.velocity[0]));
        sc.set("npc.velocity.y", Value::Num(st.velocity[1]));
        sc.set("npc.velocity.z", Value::Num(st.velocity[2]));
        // The navigator has no target entity here: follow targets are players by UUID, read as a position only.
        sc.set("target.exists", Value::Bool(false));
        sc.set("target.x", Value::Num(0.0));
        sc.set("target.y", Value::Num(0.0));
        sc.set("target.z", Value::Num(0.0));
        sc.set("target.distance", Value::Num(f64::MAX));
        sc.set("target.health", Value::Num(0.0));
        sc.set("nav.navigating", Value::Bool(self.is_navigating(id)));
        sc.set("nav.paused", Value::Bool(false));
        let dim = self.dimension_of(id);
        let mut near10 = 0.0;
        let mut nearest = f64::MAX;
        if let (Some(a), Some(dim)) = (&at, &dim) {
            for p in self.online.iter().filter(|p| &p.dimension == dim) {
                let d = look::dist2([a.x, a.y, a.z], p.pos).sqrt();
                if d <= 10.0 * 3f64.sqrt() {
                    near10 += 1.0;
                }
                if d <= 20.0 && d < nearest {
                    nearest = d;
                }
            }
        }
        // Players only: the world's other entities are not known to this mod.
        sc.set("nearby.count", Value::Num(near10));
        sc.set("nearby.player", Value::Bool(nearest < f64::MAX));
        sc.set("nearby.player.distance", Value::Num(if nearest < f64::MAX { nearest } else { 0.0 }));
        sc.inv = npc.inventory_stacks().into_iter().map(|(_, s)| (s.id, s.count)).collect();
        sc.hand = npc.equipment().into_iter().find(|e| e.slot == "mainhand").map(|e| format!("minecraft:{}", e.item.trim_start_matches("minecraft:")));
    }

    /// What expressions did to the NPC's inventory and equipment (`inv.*`).
    pub(super) fn apply_fx(&mut self, id: i32, fx: Vec<Fx>) {
        if fx.is_empty() {
            return;
        }
        let mut respawn = false;
        {
            let Some(npc) = self.saves.get_mut(id) else { return };
            for f in fx {
                match f {
                    Fx::InvAdd { id: item, count } => {
                        let mut slots = npc.inventory_stacks();
                        let used: std::collections::BTreeSet<i64> = slots.iter().map(|(s, _)| *s).collect();
                        let free = (0..72).find(|s| !used.contains(s)).unwrap_or(0);
                        slots.push((free, StackData::new(&item, count)));
                        npc.set_inventory(slots);
                    }
                    Fx::InvRemove { id: item, count } => {
                        let mut left = count;
                        let mut slots = npc.inventory_stacks();
                        for (_, s) in slots.iter_mut() {
                            if left > 0 && s.id == item {
                                let take = s.count.min(left);
                                s.count -= take;
                                left -= take;
                            }
                        }
                        slots.retain(|(_, s)| s.count > 0);
                        npc.set_inventory(slots);
                    }
                    Fx::InvClear => npc.set_inventory(Vec::new()),
                    Fx::SetHand { id: item, count } => {
                        npc.set_equipment("hand", Some((item.as_str(), count)));
                        respawn = true;
                    }
                    Fx::SetOffhand { id: item, count } => {
                        npc.set_equipment("off_hand", Some((item.as_str(), count)));
                        respawn = true;
                    }
                    Fx::Equip { slot, id: item, count } => {
                        if let Some(key) = equipment_key(&slot) {
                            npc.set_equipment(key, Some((item.as_str(), count)));
                            respawn = true;
                        }
                    }
                }
            }
        }
        self.mark_dirty();
        if respawn {
            self.respawn(id);
        }
    }

    /// One tick of every NPC's behavior tree (`BehaviorTrait`: the root is a behavior of the default controller).
    pub(super) fn behavior_tick(&mut self) {
        let ids: Vec<i32> = self.saves.npcs.iter().filter(|n| n.has_trait("behavior")).map(|n| n.id).collect();
        let gone: Vec<i32> = self.behaviors.keys().copied().filter(|id| !ids.contains(id)).collect();
        for id in gone {
            self.behaviors.remove(&id);
            self.signals.clear(id);
            self.live_stats.remove(&id);
        }
        for id in ids {
            self.run_behavior(id);
        }
    }

    fn run_behavior(&mut self, id: i32) {
        let data = self.saves.get(id).and_then(|n| n.trait_data("behavior")).cloned();
        let mut run = match self.behaviors.remove(&id) {
            Some(r) => r,
            None => BehaviorRun::new(data.as_ref().and_then(|d| d.get("memory"))),
        };
        if !self.is_spawned(id) {
            // `onDespawn`: the root is removed from the controller.
            if run.running {
                run.running = false;
                if let Some(t) = run.tree.as_mut() {
                    let mut sig = std::mem::take(&mut self.signals);
                    {
                        let mut env = BehEnv { w: self, id };
                        let mut cx = Cx { env: &mut env, sc: &mut run.scope, sig: &mut sig, npc: id };
                        t.reset_root(&mut cx);
                    }
                    self.signals = sig;
                }
            }
            self.signals.clear(id);
            self.behaviors.insert(id, run);
            return;
        }
        let tree_node = data.as_ref().and_then(|d| d.get("tree")).cloned();
        if run.src != tree_node {
            run.tree = tree_node.as_ref().and_then(Tree::from_tree_node);
            run.src = tree_node;
            run.running = false;
            self.signals.clear(id);
        }
        if run.tree.is_none() {
            self.behaviors.insert(id, run);
            return;
        }
        self.fill_scope(id, &mut run.scope);
        let mut sig = std::mem::take(&mut self.signals);
        let status = {
            let mut env = BehEnv { w: self, id };
            let mut cx = Cx { env: &mut env, sc: &mut run.scope, sig: &mut sig, npc: id };
            let tree = run.tree.as_mut().expect("checked above");
            if !run.running && !tree.should_execute(&mut cx) { None } else { Some(tree.run_root(&mut cx)) }
        };
        self.signals = sig;
        if let Some(s) = status {
            run.running = s == Status::Running;
        }
        let fx = std::mem::take(&mut run.scope.effects);
        self.apply_fx(id, fx);
        if run.scope.memory != run.saved {
            run.saved = run.scope.memory.clone();
            if let Some(npc) = self.saves.get_mut(id) {
                npc.map.map_at(&["traits", "behavior"]).set("memory", run.saved.save_to());
            }
            self.mark_dirty();
        }
        self.behaviors.insert(id, run);
    }

    /// `/npc behavior <file>`: the tree of `behaviors/<file>` becomes the NPC's (`BehaviorTrait.applyBehaviorsFromFile`).
    /// Err is the reason it was refused.
    pub fn apply_behavior_file(&mut self, id: i32, file: &str) -> Result<(), String> {
        let dir = self
            .saves_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|d| d.join("behaviors"))
            .ok_or_else(|| "no data folder".to_owned())?;
        // The file must sit directly in `behaviors` (`f.getParentFile().equals(src)`).
        if file.is_empty() || file.contains('/') || file.contains('\\') || file == "." || file == ".." {
            return Err("invalid file".to_owned());
        }
        let text = std::fs::read_to_string(dir.join(file)).map_err(|_| "invalid file".to_owned())?;
        let doc = crate::horizon_citizens_core::yaml::parse(&text).map_err(|e| e.to_string())?;
        self.apply_behavior_tree(id, doc)
    }

    /// Sets the NPC's tree (the whole map of a behavior file) and starts it over.
    pub fn apply_behavior_tree(&mut self, id: i32, tree: Node) -> Result<(), String> {
        let npc = self.saves.get_mut(id).ok_or_else(|| "no such NPC".to_owned())?;
        npc.add_trait_name("behavior");
        npc.map.map_at(&["traits", "behavior"]).set("tree", tree);
        self.mark_dirty();
        self.behaviors.remove(&id);
        self.signals.clear(id);
        Ok(())
    }

    /// `/npc eval`: the value of an expression in the NPC's scope (an empty one without an NPC), as text.
    pub fn eval_text(&mut self, npc: Option<i32>, text: &str) -> String {
        let markup = expr::default_markup(text);
        let value = expr::ExprValue::parse(&markup);
        let mut sc = Scope::new();
        // `NPCExpressionScope` has no memory: `mem.*` and `list.*` answer 0.
        sc.no_memory = true;
        match npc {
            Some(id) => self.fill_scope(id, &mut sc),
            None => sc.no_inv = true,
        }
        let out = value.evaluate(&mut sc).java_text();
        if let Some(id) = npc {
            let fx = std::mem::take(&mut sc.effects);
            self.apply_fx(id, fx);
        }
        out
    }

    /// `list --filter`: whether the boolean expression holds in the scope of `npc` (the selected NPC, none: no NPC).
    pub fn filter_holds(&mut self, npc: Option<i32>, text: &str) -> Result<bool, String> {
        let value = expr::ExprValue::parse(&expr::default_markup(text));
        let mut sc = Scope::new();
        sc.no_memory = true;
        match npc {
            Some(id) => self.fill_scope(id, &mut sc),
            None => sc.no_inv = true,
        }
        Ok(value.as_bool(&mut sc))
    }

    /// Whether an NPC runs a tree now (for tests and the info output).
    pub fn behavior_running(&self, id: i32) -> bool {
        self.behaviors.get(&id).is_some_and(|r| r.running)
    }

    /// The memory of an NPC's tree.
    pub fn behavior_memory(&self, id: i32) -> Option<Memory> {
        self.behaviors.get(&id).map(|r| r.scope.memory.clone())
    }

    /// The signals registry (for tests).
    pub fn behavior_signals(&mut self) -> &mut Signals {
        &mut self.signals
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_citizens_core::config::Config;
    use crate::horizon_citizens_core::saves::Saves;
    use crate::horizon_citizens_core::yaml;

    fn world() -> World {
        let mut cfg = Config::default();
        cfg.skins.fetch = false;
        World::new(cfg, Saves::empty(), None)
    }

    fn loc() -> Location {
        Location { world: "world".into(), x: 0.5, y: 64.0, z: 0.5, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 }
    }

    fn spawned(w: &mut World, tree: &str) -> i32 {
        let id = w.create("Bob", "PLAYER", loc(), None);
        let entity = 700 + id as u32;
        let uuid = w.saves.get(id).and_then(|n| n.uuid());
        w.on_entity_join("minecraft:overworld", entity, uuid, true);
        w.apply_behavior_tree(id, yaml::parse(tree).expect("yaml")).expect("applied");
        w.take_ops();
        id
    }

    fn consoles(w: &mut World) -> Vec<String> {
        w.take_ops()
            .into_iter()
            .filter_map(|o| match o {
                super::super::Op::Console(l) => Some(l),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tree_runs_every_tick_and_says_through_the_npc() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': say hello there\n");
        w.behavior_tick();
        let lines = consoles(&mut w);
        assert!(lines.iter().any(|l| l.starts_with("npc speak \"hello there\" --id")), "{lines:?}");
        assert!(!w.behavior_running(id));
    }

    #[test]
    fn memory_is_saved_on_the_npc() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': set mood 4\n  '1': say hi\n");
        w.behavior_tick();
        let saved = w.saves.get(id).and_then(|n| n.trait_data("behavior")).and_then(|d| d.get("memory")).cloned();
        let saved = saved.expect("memory is saved");
        assert_eq!(saved.get("mood").and_then(Node::as_f64).or_else(|| saved.get("mood").and_then(|n| n.as_str().and_then(|s| s.parse().ok()))), Some(4.0));
    }

    #[test]
    fn the_scope_has_the_npc_values() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': say x\n");
        let mut sc = Scope::new();
        w.fill_scope(id, &mut sc);
        assert_eq!(sc.vars.get("npc.x"), Some(&Value::Num(0.5)));
        assert_eq!(sc.vars.get("npc.spawned"), Some(&Value::Bool(true)));
        assert_eq!(sc.vars.get("nearby.player"), Some(&Value::Bool(false)));
    }

    #[test]
    fn eval_text_uses_the_npc_scope() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': say x\n");
        assert_eq!(w.eval_text(Some(id), "npc.y + 1"), "65.0");
        assert_eq!(w.eval_text(Some(id), "mem.set('a', 3)"), "0.0", "the eval scope has no memory");
        assert_eq!(w.eval_text(None, "2 * 4"), "8.0");
        assert_eq!(w.eval_text(None, "math.max(1, 5) == 5"), "1.0");
    }

    #[test]
    fn inventory_expressions_change_the_npc() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': say x\n");
        w.eval_text(Some(id), "inv.add(item.from_component('diamond', 5))");
        let stacks = w.saves.get(id).unwrap().inventory_stacks();
        assert_eq!(stacks.len(), 1);
        assert_eq!((stacks[0].1.id.as_str(), stacks[0].1.count), ("minecraft:diamond", 5));
        assert_eq!(w.eval_text(Some(id), "inv.count('diamond')"), "5.0");
        w.eval_text(Some(id), "inv.remove('diamond', 2)");
        assert_eq!(w.saves.get(id).unwrap().inventory_stacks()[0].1.count, 3);
    }

    #[test]
    fn changing_the_tree_starts_a_new_one() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': say first\n");
        w.behavior_tick();
        consoles(&mut w);
        w.apply_behavior_tree(id, yaml::parse("sequence:\n  '0': say second\n").unwrap()).unwrap();
        w.behavior_tick();
        let lines = consoles(&mut w);
        assert!(lines.iter().any(|l| l.contains("second")), "{lines:?}");
    }

    #[test]
    fn a_despawned_npc_runs_nothing() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': say hi\n");
        w.despawn(id, super::super::DespawnReason::Command);
        w.take_ops();
        w.behavior_tick();
        assert!(consoles(&mut w).iter().all(|l| !l.contains("speak")));
    }

    #[test]
    fn walkto_uses_the_navigator() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': walkto 5 64 0\n  '1': say there\n");
        w.behavior_tick();
        assert!(w.is_navigating(id));
        assert!(w.behavior_running(id));
    }

    #[test]
    fn no_behavior_file_outside_the_folder() {
        let mut w = world();
        let id = spawned(&mut w, "sequence:\n  '0': say x\n");
        assert!(w.apply_behavior_file(id, "../saves.yml").is_err());
        assert!(w.apply_behavior_file(id, "a/b.yml").is_err());
    }
}
