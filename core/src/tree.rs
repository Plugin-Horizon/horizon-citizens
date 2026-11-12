//! Path helpers over the vendored YAML tree (`yaml::Node`), in the shape the NPC code uses:
//! `at` / `put` take a path as a slice of keys (no dots inside a key).

use crate::yaml::Node;

pub trait TreeExt {
    /// The node at `path` (`None` when a key is missing or a step is not a map).
    fn at(&self, path: &[&str]) -> Option<&Node>;
    /// Sets the node at `path`, creating the maps on the way (a non-map on the way is replaced).
    fn put(&mut self, path: &[&str], value: Node);
    /// The node at `path` as a map, created empty when missing (for editing its keys).
    fn map_at(&mut self, path: &[&str]) -> &mut Node;
    /// Removes the key at `path`; the removed node.
    fn drop_path(&mut self, path: &[&str]) -> Option<Node>;
    /// The scalar's text (numbers and booleans too), `None` for null, lists and maps.
    fn text(&self) -> Option<String>;
    /// This node as a map (itself when it is one).
    fn as_map(&self) -> Option<&Node>;
    /// A sequence's items.
    fn as_seq(&self) -> Option<&[Node]>;
    /// This node as a mutable map (itself when it is one).
    fn as_map_mut(&mut self) -> Option<&mut Node>;
}

impl TreeExt for Node {
    fn at(&self, path: &[&str]) -> Option<&Node> {
        path.iter().try_fold(self, |node, key| node.get(key))
    }

    fn put(&mut self, path: &[&str], value: Node) {
        match path {
            [] => *self = value,
            [last] => self.set(last, value),
            [first, rest @ ..] => {
                if self.get(first).is_none_or(|n| !n.is_map()) {
                    self.set(first, Node::map());
                }
                if let Some(next) = self.get_mut(first) {
                    next.put(rest, value);
                }
            }
        }
    }

    fn map_at(&mut self, path: &[&str]) -> &mut Node {
        match path {
            [] => {
                if !self.is_map() {
                    *self = Node::map();
                }
                self
            }
            [first, rest @ ..] => {
                if self.get(first).is_none_or(|n| !n.is_map()) {
                    self.set(first, Node::map());
                }
                let next = self.get_mut(first).expect("just set");
                next.map_at(rest)
            }
        }
    }

    fn drop_path(&mut self, path: &[&str]) -> Option<Node> {
        match path {
            [] => None,
            [last] => self.remove(last),
            [first, rest @ ..] => self.get_mut(first)?.drop_path(rest),
        }
    }

    fn text(&self) -> Option<String> {
        self.as_str().map(str::to_owned)
    }

    fn as_map(&self) -> Option<&Node> {
        self.is_map().then_some(self)
    }

    fn as_seq(&self) -> Option<&[Node]> {
        self.is_seq().then(|| self.items())
    }

    fn as_map_mut(&mut self) -> Option<&mut Node> {
        self.is_map().then_some(self)
    }
}
