//! `saves.yml`: Citizens' NPC store. `last-created-npc-id` and the `npc` map (`'<id>': {...}`)
//! are read into [`NpcData`]; every other key is kept as the file has it. The file is written
//! through the vendored writer, which gives back the same bytes for a file Citizens wrote, and
//! written whole, through a temporary file and a rename.
//!
//! Nothing is dropped on the way: a key of the `npc` map that is not an id (or whose value is
//! not a map) is kept where it is. An id whose NPC was removed is taken out.

use crate::npc::NpcData;
use crate::tree::TreeExt;
use crate::yaml::{self, Document, Entry, Flow, Node, Style};
use std::path::Path;

pub const HEADER: &str = "Citizens NPC Storage";

#[derive(Clone, Debug, PartialEq)]
pub struct Saves {
    /// The file as read (comments, key order, quoting). `npc` and `last-created-npc-id` are
    /// written from the fields below when [`Saves::to_yaml`] runs.
    doc: Document,
    pub last_created_id: i32,
    /// In file order.
    pub npcs: Vec<NpcData>,
}

impl Saves {
    pub fn parse(text: &str) -> Result<Saves, yaml::Error> {
        let doc = yaml::parse_document(text)?;
        let last_created_id = doc
            .root
            .get("last-created-npc-id")
            .and_then(Node::as_i64)
            .unwrap_or(-1) as i32;
        let mut npcs = Vec::new();
        if let Some(npc) = doc.root.get("npc") {
            for e in npc.entries() {
                let (Ok(id), true) = (e.key.trim().parse::<i32>(), e.value.is_map()) else {
                    continue;
                };
                npcs.push(NpcData {
                    id,
                    map: e.value.clone(),
                });
            }
        }
        Ok(Saves {
            doc,
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
        let mut root = Node::map();
        root.set("last-created-npc-id", Node::int(-1));
        root.set("npc", Node::map());
        Saves {
            doc: Document {
                leading: vec![format!("# {HEADER}")],
                root,
                trailing: Vec::new(),
            },
            last_created_id: -1,
            npcs: Vec::new(),
        }
    }

    pub fn to_yaml(&self) -> String {
        let mut doc = self.doc.clone();
        let root = &mut doc.root;
        if root.get("last-created-npc-id").is_none() && self.last_created_id != -1 {
            if let Node::Map { entries, .. } = root {
                entries.insert(0, entry("last-created-npc-id", Node::int(-1)));
            }
        }
        if root.get("last-created-npc-id").is_some() {
            root.set("last-created-npc-id", Node::int(i64::from(self.last_created_id)));
        }
        if root.get("npc").is_some() || !self.npcs.is_empty() {
            let npc = root.map_at(&["npc"]);
            self.write_npcs(npc);
        }
        yaml::write_document(&doc)
    }

    /// Updates the `npc` map in place: known ids get their NPC's map, ids that are gone are
    /// removed, other keys stay, new NPCs are appended.
    fn write_npcs(&self, npc: &mut Node) {
        let Node::Map { entries, flow } = npc else {
            *npc = Node::map();
            return self.write_npcs(npc);
        };
        let mut written: Vec<i32> = Vec::new();
        entries.retain_mut(|e| match e.key.trim().parse::<i32>() {
            Ok(id) if e.value.is_map() => match self.npcs.iter().find(|n| n.id == id) {
                Some(n) => {
                    written.push(id);
                    e.value = n.map.clone();
                    true
                }
                None => false,
            },
            _ => true,
        });
        for n in &self.npcs {
            if !written.contains(&n.id) {
                entries.push(Entry {
                    key: n.id.to_string(),
                    key_style: Style::Single,
                    value: n.map.clone(),
                    comments: Vec::new(),
                    inline_comment: None,
                });
            }
        }
        if !entries.is_empty() && *flow != Flow::Block {
            *flow = Flow::Block;
        }
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

fn entry(key: &str, value: Node) -> Entry {
    Entry {
        key: key.to_owned(),
        key_style: Style::Plain,
        value,
        comments: Vec::new(),
        inline_comment: None,
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
    fn keys_that_are_not_ids_are_kept_not_dropped() {
        let text = "# Citizens NPC Storage\nlast-created-npc-id: 3\nnpc:\n  '3':\n    name: A\n    uuid: 00000000-0000-4000-8000-000000000003\n  'note':\n    text: kept\n  '4': 7\n";
        let s = Saves::parse(text).unwrap();
        assert_eq!(s.npcs.len(), 1);
        assert_eq!(s.to_yaml(), text, "a non-id key and a non-map value survive a write");
    }

    #[test]
    fn top_level_keys_are_kept_in_place() {
        let text = "# Citizens NPC Storage\nlast-created-npc-id: 3\nother-setting: 'yes please'\nnpc:\n  '3':\n    name: A\n    uuid: 00000000-0000-4000-8000-000000000003\n";
        let s = Saves::parse(text).unwrap();
        assert_eq!(s.to_yaml(), text);
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
    fn removed_npcs_leave_the_file() {
        let mut s = Saves::parse(FILE).unwrap();
        s.remove(3);
        let text = s.to_yaml();
        assert!(!text.contains("  '3':"));
        assert!(text.contains("  '12':\n"));
    }

    #[test]
    fn empty_and_missing_files() {
        assert_eq!(Saves::parse("").unwrap().npcs.len(), 0);
        let s = Saves::empty();
        assert_eq!(s.next_id(), 0);
        let mut s = Saves::parse(&s.to_yaml()).unwrap();
        s.insert(NpcData::new(s.next_id(), 1, "A", "PLAYER"));
        let text = s.to_yaml();
        assert!(text.contains("last-created-npc-id: 0\n"), "{text}");
        assert!(text.contains("npc:\n  '0':\n"), "{text}");
        assert_eq!(Saves::parse(&text).unwrap().npcs.len(), 1);
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

#[cfg(test)]
mod citizens_file_tests {
    use super::*;

    /// A file in the layout Citizens writes (invented values): cosmetic slots, an invisibility
    /// potion, an item with decoded-later meta, keys the port does not read.
    fn fixture() -> String {
        format!(
            "# Citizens NPC Storage
last-created-npc-id: 5
npc:
  '5':
    metadata:
      protected: true
      nameplate-visible: 'false'
    name: Fixture
    navigator:
      avoidwater: false
      speedmodifier: 1.5
    traits:
      equipment:
        cosmetic_chestplate:
          type: air
          amount: 1
        hand:
          type: DIAMOND_SWORD
          amount: 1
          meta: '{}'
      followtrait: ''
      potioneffects:
        persistent:
          invisibility:
            type: INVISIBILITY
            duration: -1
            amplifier: 0
            ambient: false
            particles: false
            icon: false
      traitnames: equipment,potioneffects
    uuid: 00000000-0000-4000-8000-000000000005
",
            crate::items::fixtures::BLADE
        )
    }

    #[test]
    fn a_citizens_file_comes_back_byte_for_byte() {
        let text = fixture();
        let s = Saves::parse(&text).unwrap();
        assert_eq!(s.npcs.len(), 1);
        assert_eq!(s.to_yaml(), text);
    }

    #[test]
    fn an_edit_keeps_the_cosmetic_slots_and_the_unread_keys() {
        let text = fixture();
        let mut s = Saves::parse(&text).unwrap();
        s.get_mut(5).unwrap().set_equipment("hand", None);
        let out = s.to_yaml();
        assert!(!out.contains("DIAMOND_SWORD"), "the hand is emptied");
        assert!(out.contains("cosmetic_chestplate:\n          type: air\n"), "{out}");
        assert!(out.contains("followtrait: ''\n"), "{out}");
        assert!(out.contains("speedmodifier: 1.5\n"), "{out}");
        assert!(out.contains("invisibility:\n"), "{out}");
    }
}
