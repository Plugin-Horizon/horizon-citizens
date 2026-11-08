//! `saves.yml`: Citizens' NPC store. `last-created-npc-id` and `npc.<id>` maps; other top-level
//! keys are kept. Written whole, through a temporary file and a rename.

use crate::npc::NpcData;
use crate::yaml::{self, Key, Map, Node};
use std::path::Path;

pub const HEADER: &str = "Citizens NPC Storage";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saves {
    /// The top level as read, with `npc` and `last-created-npc-id` taken out (kept in place by
    /// [`Saves::to_yaml`]).
    rest: Map,
    pub last_created_id: i32,
    /// In file order.
    pub npcs: Vec<NpcData>,
}

impl Saves {
    pub fn parse(text: &str) -> Result<Saves, yaml::Error> {
        let mut rest = yaml::parse(text)?;
        let last_created_id = rest
            .get("last-created-npc-id")
            .and_then(Node::as_i64)
            .unwrap_or(-1) as i32;
        let mut npcs = Vec::new();
        if let Some(Node::Map(m)) = rest.get("npc") {
            for (k, v) in &m.entries {
                let (Ok(id), Node::Map(map)) = (k.name.trim().parse::<i32>(), v) else {
                    continue;
                };
                npcs.push(NpcData { id, map: map.clone() });
            }
        }
        // The keys keep their places for writing; missing ones are added only when needed.
        if rest.get("npc").is_some() {
            rest.set("npc", Node::map());
        }
        Ok(Saves {
            rest,
            last_created_id,
            npcs,
        })
    }

    pub fn load(path: &Path) -> Result<Saves, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Saves::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Saves::empty()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn empty() -> Saves {
        let mut rest = Map::default();
        rest.set("last-created-npc-id", Node::int(-1));
        rest.set("npc", Node::map());
        Saves {
            rest,
            last_created_id: -1,
            npcs: Vec::new(),
        }
    }

    pub fn to_yaml(&self) -> String {
        let mut root = self.rest.clone();
        if root.get("last-created-npc-id").is_some() || self.last_created_id != -1 {
            if root.get("last-created-npc-id").is_none() {
                root.entries
                    .insert(0, (Key::new("last-created-npc-id"), Node::int(-1)));
            }
            root.set("last-created-npc-id", Node::int(i64::from(self.last_created_id)));
        }
        if root.get("npc").is_some() || !self.npcs.is_empty() {
            let mut npc = Map::default();
            for n in &self.npcs {
                npc.entries
                    .push((Key::new(&n.id.to_string()), Node::Map(n.map.clone())));
            }
            root.set("npc", Node::Map(npc));
        }
        yaml::emit(&root, &[HEADER])
    }

    /// Writes the file whole: a temporary file beside it, then a rename.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = path.with_extension("yml.tmp");
        std::fs::write(&tmp, self.to_yaml()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn get(&self, id: i32) -> Option<&NpcData> {
        self.npcs.iter().find(|n| n.id == id)
    }
    pub fn get_mut(&mut self, id: i32) -> Option<&mut NpcData> {
        self.npcs.iter_mut().find(|n| n.id == id)
    }
    pub fn by_uuid(&self, uuid: u128) -> Option<&NpcData> {
        self.npcs.iter().find(|n| n.uuid() == Some(uuid))
    }

    /// The next id, as Citizens hands them out: one past the highest given so far.
    pub fn next_id(&self) -> i32 {
        let highest = self.npcs.iter().map(|n| n.id).max().unwrap_or(-1);
        self.last_created_id.max(highest) + 1
    }

    pub fn insert(&mut self, npc: NpcData) {
        self.last_created_id = self.last_created_id.max(npc.id);
        self.npcs.retain(|n| n.id != npc.id);
        self.npcs.push(npc);
    }

    pub fn remove(&mut self, id: i32) -> Option<NpcData> {
        let i = self.npcs.iter().position(|n| n.id == id)?;
        Some(self.npcs.remove(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# Citizens NPC Storage
last-created-npc-id: 12
npc:
  '3':
    traits:
      type: PLAYER
    name: First
    uuid: 00000000-0000-4000-8000-000000000003
  '12':
    traits:
      type: SNOW_GOLEM
      spawned: false
    name: Second
    uuid: 00000000-0000-4000-8000-00000000000c
";

    #[test]
    fn round_trip_is_exact() {
        let s = Saves::parse(FILE).unwrap();
        assert_eq!(s.npcs.len(), 2);
        assert_eq!(s.last_created_id, 12);
        assert_eq!(s.to_yaml(), FILE);
    }

    #[test]
    fn ids_continue_after_the_last_created() {
        let mut s = Saves::parse(FILE).unwrap();
        s.remove(12);
        assert_eq!(s.next_id(), 13, "a removed id is not handed out again");
        let n = NpcData::new(s.next_id(), 99, "Third", "PLAYER");
        s.insert(n);
        let text = s.to_yaml();
        assert!(text.contains("last-created-npc-id: 13"));
        assert!(text.contains("  '13':\n"));
        assert_eq!(Saves::parse(&text).unwrap().npcs.len(), 2);
    }

    #[test]
    fn empty_and_missing_files() {
        assert_eq!(Saves::parse("").unwrap().npcs.len(), 0);
        let s = Saves::empty();
        assert_eq!(s.next_id(), 0);
        assert_eq!(
            s.to_yaml(),
            "# Citizens NPC Storage\nlast-created-npc-id: -1\nnpc: {\n  }\n"
        );
        // An empty store as Citizens writes it stays that way, and takes NPCs.
        let mut s = Saves::parse("# Citizens NPC Storage\n{\n  }\n").unwrap();
        assert_eq!(s.to_yaml(), "# Citizens NPC Storage\n{\n  }\n");
        s.insert(NpcData::new(s.next_id(), 1, "A", "PLAYER"));
        let text = s.to_yaml();
        assert!(text.starts_with("# Citizens NPC Storage\nlast-created-npc-id: 0\nnpc:\n  '0':\n"), "{text}");
    }

    #[test]
    fn saves_through_a_temporary_file() {
        let dir = std::env::temp_dir().join(format!("citizens-core-test-{}", std::process::id()));
        let path = dir.join("saves.yml");
        let s = Saves::parse(FILE).unwrap();
        s.save(&path).unwrap();
        assert_eq!(Saves::load(&path).unwrap(), s);
        assert!(!path.with_extension("yml.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
