//! Citizens' behavior trees (`BehaviorTrait`, `BehaviorTreeParser`, the API's `ai.tree` package and its
//! `BehaviorRegistry` defaults, `CitizensBehaviorRegistry`).
//!
//! A tree is parsed from the `traits.behavior.tree` map of an NPC (or a `behaviors/<file>.yml`). Containers:
//! `sequence`/`seq`, `random`/`selector`, `parallel`, `timeout <ticks>`, `invert`, `loop <condition>`, `eval`
//! (one expression per child), `if <condition>` with an `else` sibling. Leaves: `wait`, `emit_signal`,
//! `emit_signal_to`, `emit_global_signal`, `wait_for_signal`, `succeed`, `fail`, `repeat`, `set`,
//! `wait_until`, `cooldown`, `forget`, `clear_memory`, `walkto`, `wander`, `break_block`, `print`, `say`,
//! `look`, `teleport`, a `/command` line and a `` `expression` `` line.
//!
//! Where the original's code cannot do what its own comments say, the intended behaviour is built: `wait`
//! takes its duration from an inline argument too, a `parallel` container really runs its children, `wander`
//! picks a destination (the original never calls the goal's `shouldExecute`), and an `if` followed by a
//! sibling that is not an `else` keeps that sibling.
//!
//! The tree never calls the game: everything it does to the NPC goes through [`Env`].

use crate::horizon_citizens_core::expr::{self, Compiled, ExprValue, Scope, Value};
use crate::horizon_citizens_core::tree::TreeExt;
use crate::horizon_citizens_core::yaml::Node;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Failure,
    ResetAndRemove,
    Running,
    Success,
}

// ---- the data key the parser reads --------------------------------------------------------------

/// A `DataKey` as the parser reads it: a name, a scalar value and sub keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Key {
    pub name: String,
    pub value: Option<String>,
    pub subs: Vec<Key>,
}

impl Key {
    /// A YAML node as a key; a list reads as sub keys named by position.
    pub fn from_node(name: &str, n: &Node) -> Key {
        let mut k = Key { name: name.to_owned(), value: None, subs: Vec::new() };
        if n.is_map() {
            for e in n.entries() {
                k.subs.push(Key::from_node(&e.key, &e.value));
            }
        } else if n.is_seq() {
            for (i, item) in n.items().iter().enumerate() {
                k.subs.push(Key::from_node(&i.to_string(), item));
            }
        } else {
            k.value = n.text();
        }
        k
    }

    fn has_subs(&self) -> bool {
        !self.subs.is_empty()
    }

    /// `getString(child)`.
    fn string(&self, child: &str) -> Option<String> {
        self.subs.iter().find(|s| s.name == child).and_then(|s| s.value.clone())
    }

    /// `getIntegerSubKeys`: the sub keys named by whole numbers, in numeric order.
    fn int_subs(&self) -> Vec<&Key> {
        let mut v: Vec<(i64, &Key)> = self.subs.iter().filter_map(|s| s.name.trim().parse::<i64>().ok().map(|n| (n, s))).collect();
        v.sort_by_key(|(n, _)| *n);
        v.into_iter().map(|(_, k)| k).collect()
    }
}

// ---- what the tree asks of the NPC ----------------------------------------------------------------

/// The NPC and the world as the tree sees them.
pub trait Env {
    fn spawned(&self) -> bool;
    fn navigating(&self) -> bool;
    /// `Navigator.setTarget` with a speed modifier (1 = the NPC's own speed).
    fn walk_to(&mut self, target: [f64; 3], speed: Option<f64>);
    /// Picks a random destination within `radius` blocks and starts walking there; false when none was found.
    fn wander(&mut self, radius: i64) -> bool;
    fn look_at(&mut self, p: [f64; 3]);
    fn teleport(&mut self, world: Option<&str>, p: [f64; 3]);
    /// `Util.runCommand`: a command line as the console runs it (as the NPC itself with `player`).
    fn command(&mut self, line: &str, op: bool, player: bool);
    fn say(&mut self, msg: &str);
    fn log(&mut self, msg: &str);
    fn distance_to(&self, p: [f64; 3]) -> f64;
    fn break_block(&mut self, p: [i64; 3]);
    /// An NPC by id or UUID text.
    fn find_npc(&self, id_or_uuid: &str) -> Option<i32>;
    fn now_ms(&self) -> u64;
    fn random(&mut self) -> f64;
}

/// `BehaviorSignals`: who listens to which signal, and which listeners were signalled.
#[derive(Clone, Debug, Default)]
pub struct Signals {
    listeners: BTreeMap<i32, Vec<(String, u64)>>,
    fired: BTreeSet<u64>,
    next: u64,
}

impl Signals {
    pub fn listen(&mut self, npc: i32, signal: &str) -> u64 {
        self.next += 1;
        self.listeners.entry(npc).or_default().push((signal.to_owned(), self.next));
        self.next
    }

    pub fn unlisten(&mut self, npc: i32, signal: &str, token: u64) {
        if let Some(l) = self.listeners.get_mut(&npc) {
            l.retain(|(s, t)| !(s == signal && *t == token));
            if l.is_empty() {
                self.listeners.remove(&npc);
            }
        }
        self.fired.remove(&token);
    }

    pub fn emit_to(&mut self, npc: i32, signal: &str) {
        if let Some(l) = self.listeners.get(&npc) {
            for (s, t) in l {
                if s == signal {
                    self.fired.insert(*t);
                }
            }
        }
    }

    pub fn emit_global(&mut self, signal: &str) {
        for l in self.listeners.values() {
            for (s, t) in l {
                if s == signal {
                    self.fired.insert(*t);
                }
            }
        }
    }

    pub fn was_fired(&self, token: u64) -> bool {
        self.fired.contains(&token)
    }

    /// `clearListeners`.
    pub fn clear(&mut self, npc: i32) {
        if let Some(l) = self.listeners.remove(&npc) {
            for (_, t) in l {
                self.fired.remove(&t);
            }
        }
    }
}

/// What a run of the tree works with.
pub struct Cx<'a> {
    pub env: &'a mut dyn Env,
    pub sc: &'a mut Scope,
    pub sig: &'a mut Signals,
    pub npc: i32,
}

// ---- the tree -------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Cond {
    Const(bool),
    Expr(Compiled),
    Var(String),
}

fn parse_condition(s: &str) -> Cond {
    if expr::is_expression(s) {
        return match expr::compile_markup(s) {
            Ok(c) => Cond::Expr(c),
            Err(e) => {
                eprintln!("[citizens] behavior condition {s}: {e}");
                Cond::Const(false)
            }
        };
    }
    if s.eq_ignore_ascii_case("true") || s == "1" {
        return Cond::Const(true);
    }
    if s.eq_ignore_ascii_case("false") || s == "0" {
        return Cond::Const(false);
    }
    Cond::Var(s.to_owned())
}

fn eval_cond(c: &Cond, sc: &mut Scope) -> bool {
    match c {
        Cond::Const(b) => *b,
        Cond::Expr(e) => e.eval_bool(sc),
        Cond::Var(name) => match sc.vars.get(name.as_str()).or_else(|| sc.locals.get(name.as_str())) {
            Some(Value::Bool(b)) => *b,
            Some(Value::Num(n)) => *n != 0.0,
            Some(_) => true,
            None => false,
        },
    }
}

#[derive(Clone, Debug, Default)]
struct Comp {
    behaviors: Vec<usize>,
    parallel: Vec<usize>,
    executing: Vec<usize>,
}

#[derive(Clone, Debug)]
enum Leaf {
    Command(ExprValue),
    Expr(ExprValue),
    Say(ExprValue),
    Print(ExprValue),
    Look(ExprValue, ExprValue, ExprValue),
    Teleport(ExprValue, ExprValue, ExprValue, Option<ExprValue>),
    Emit(ExprValue),
    EmitTo(ExprValue, ExprValue),
    EmitGlobal(ExprValue),
    Succeed,
    Fail,
    Set(String, ExprValue),
    Cooldown(String, ExprValue),
    Forget(String),
    ClearMemory,
}

#[derive(Clone, Debug)]
enum K {
    Empty,
    Instant(Leaf),
    EvalBlock(Vec<Compiled>),
    Wait { raw: String, dur: ExprValue, remaining: i64 },
    WaitSignal { sig: ExprValue, cur: Option<(String, u64)> },
    Repeat { count: ExprValue, remaining: i64 },
    WaitUntil(ExprValue),
    Walk { x: ExprValue, y: ExprValue, z: ExprValue, speed: Option<ExprValue>, started: bool },
    Wander { radius: ExprValue, started: bool },
    BreakBlock { x: ExprValue, y: ExprValue, z: ExprValue, radius: ExprValue, waited: i64, total: i64 },
    Sequence { c: Comp, executing: Option<usize>, index: i64 },
    Selector { c: Comp, executing: Option<usize> },
    Parallel { c: Comp, started: bool },
    Timeout { child: usize, max: i64, elapsed: i64 },
    Invert { child: usize },
    Loop { child: usize, cond: Cond },
    IfElse { cond: Cond, then: usize, els: Option<usize>, executing: Option<usize> },
    Coalesced { children: Vec<usize> },
    ParallelWrap { child: usize },
}

/// A parsed behavior tree.
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<K>,
    root: usize,
}

struct Parser {
    nodes: Vec<K>,
}

impl Parser {
    fn add(&mut self, k: K) -> usize {
        self.nodes.push(k);
        self.nodes.len() - 1
    }

    fn is_instant(&self, n: usize) -> bool {
        matches!(self.nodes[n], K::Instant(_) | K::EvalBlock(_))
    }

    fn comp(&self, children: Vec<usize>) -> Comp {
        let mut c = Comp::default();
        for ch in children {
            if matches!(self.nodes[ch], K::ParallelWrap { .. }) {
                c.parallel.push(ch);
            } else {
                c.behaviors.push(ch);
            }
        }
        c
    }

    fn coalesce(&mut self, list: Vec<usize>) -> Vec<usize> {
        let mut out = Vec::new();
        let mut group: Vec<usize> = Vec::new();
        for b in list {
            if self.is_instant(b) {
                group.push(b);
            } else {
                if !group.is_empty() {
                    let g = std::mem::take(&mut group);
                    out.push(if g.len() == 1 { g[0] } else { self.add(K::Coalesced { children: g }) });
                }
                out.push(b);
            }
        }
        if !group.is_empty() {
            out.push(if group.len() == 1 { group[0] } else { self.add(K::Coalesced { children: group }) });
        }
        out
    }

    fn sequence(&mut self, list: Vec<usize>) -> usize {
        let c = self.comp(list);
        self.add(K::Sequence { c, executing: None, index: -1 })
    }

    fn body_of(&mut self, key: &Key) -> usize {
        let children = self.children(key);
        if children.len() == 1 {
            children[0]
        } else {
            let list = self.coalesce(children);
            self.sequence(list)
        }
    }

    fn empty(&mut self) -> usize {
        self.add(K::Empty)
    }

    fn node(&mut self, key: &Key) -> Option<usize> {
        if key.has_subs() {
            let mut if_key = None;
            let mut else_key = None;
            for s in &key.subs {
                if s.name.starts_with("if ") {
                    if_key = Some(s);
                } else if s.name == "else" {
                    else_key = Some(s);
                }
            }
            if let Some(i) = if_key {
                return self.if_else(i, else_key);
            }
            return self.container(&key.name, key);
        }
        match key.value.as_deref() {
            Some(v) if !v.is_empty() => self.leaf(v, None),
            _ => self.leaf(&key.name, Some(key)),
        }
    }

    fn container(&mut self, name: &str, key: &Key) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        if name.starts_with("timeout ") {
            let spec = name[8..].trim();
            let ticks = ExprValue::parse(spec).as_number(&mut Scope::new()) as i64;
            let child = self.body_of(key);
            return Some(self.add(K::Timeout { child, max: ticks, elapsed: 0 }));
        }
        if lower == "invert" {
            let child = self.body_of(key);
            return Some(self.add(K::Invert { child }));
        }
        if name.starts_with("loop ") {
            let cond = parse_condition(name[5..].trim());
            let child = self.body_of(key);
            return Some(self.add(K::Loop { child, cond }));
        }
        if lower == "sequence" || lower == "seq" {
            let children = self.children(key);
            let list = self.coalesce(children);
            return Some(self.sequence(list));
        }
        if lower == "random" || lower == "selector" {
            let children = self.children(key);
            let c = self.comp(children);
            return Some(self.add(K::Selector { c, executing: None }));
        }
        if lower == "parallel" {
            let children = self.children(key);
            let wrapped: Vec<usize> = children.into_iter().map(|child| self.add(K::ParallelWrap { child })).collect();
            let c = self.comp(wrapped);
            return Some(self.add(K::Parallel { c, started: false }));
        }
        if lower == "eval" || lower.starts_with("eval ") {
            return Some(self.eval_block(key));
        }
        self.leaf(name, Some(key))
    }

    fn eval_block(&mut self, key: &Key) -> usize {
        let mut list = Vec::new();
        for sub in key.int_subs() {
            if let Some(src) = sub.value.as_deref().filter(|s| !s.is_empty()) {
                match expr::compile(src) {
                    Ok(c) => list.push(c),
                    Err(e) => eprintln!("[citizens] behavior eval {src}: {e}"),
                }
            }
        }
        self.add(K::EvalBlock(list))
    }

    /// `parseContainerChildren`, with an `if` that has no `else` after it keeping the next sibling.
    fn children(&mut self, key: &Key) -> Vec<usize> {
        let mut out = Vec::new();
        let mut if_key: Option<&Key> = None;
        for sub in key.int_subs() {
            let sub = if sub.has_subs() { &sub.subs[0] } else { sub };
            if let Some(i) = if_key.take() {
                if sub.name == "else" {
                    if let Some(c) = self.if_else(i, Some(sub)) {
                        out.push(c);
                    }
                    continue;
                }
                if let Some(c) = self.if_else(i, None) {
                    out.push(c);
                }
            }
            if sub.name.starts_with("if ") {
                if_key = Some(sub);
                continue;
            }
            if let Some(c) = self.node(sub) {
                out.push(c);
            }
        }
        if let Some(i) = if_key
            && let Some(c) = self.if_else(i, None)
        {
            out.push(c);
        }
        out
    }

    fn branch(&mut self, key: &Key) -> Option<usize> {
        if key.has_subs() {
            let mut children = Vec::new();
            for sub in key.int_subs() {
                let sub = if sub.has_subs() { &sub.subs[0] } else { sub };
                if let Some(c) = self.node(sub) {
                    children.push(c);
                }
            }
            return Some(if children.len() == 1 { children[0] } else { self.sequence(children) });
        }
        key.value.as_deref().filter(|v| !v.is_empty()).and_then(|v| self.leaf(v, None))
    }

    fn if_else(&mut self, if_key: &Key, else_key: Option<&Key>) -> Option<usize> {
        let cond = parse_condition(if_key.name[3..].trim());
        let then = match self.branch(if_key) {
            Some(t) => t,
            None => self.empty(),
        };
        let els = else_key.and_then(|k| self.branch(k));
        Some(self.add(K::IfElse { cond, then, els, executing: None }))
    }

    fn leaf(&mut self, name: &str, params: Option<&Key>) -> Option<usize> {
        if name.is_empty() {
            return Some(self.empty());
        }
        if let Some(cmd) = name.strip_prefix('/') {
            return Some(self.add(K::Instant(Leaf::Command(ExprValue::parse(cmd)))));
        }
        if name.starts_with('`') {
            return Some(self.add(K::Instant(Leaf::Expr(ExprValue::parse(name)))));
        }
        let parts = inline_args(name);
        let Some(behavior) = parts.first().cloned() else { return Some(self.empty()) };
        let args: Vec<String> = parts[1..].to_vec();
        match self.create(&behavior, params, &args) {
            Some(n) => Some(n),
            None => {
                eprintln!("[citizens] unknown behavior {behavior}");
                Some(self.empty())
            }
        }
    }

    /// `BehaviorRegistry.createBehavior`: a factory returns nothing for missing arguments.
    fn create(&mut self, name: &str, params: Option<&Key>, args: &[String]) -> Option<usize> {
        let arg = |i: usize, key: &str, default: Option<&str>| arg_or_param(args, i, key, params, default);
        let pv = |s: &str| ExprValue::parse(s);
        let k = match name.to_ascii_lowercase().as_str() {
            "wait" => {
                // The original reads only the key's own value; an inline `wait 40` is read as well here.
                let raw = args.first().cloned().or_else(|| params.and_then(|p| p.value.clone())).unwrap_or_else(|| "20".to_owned());
                K::Wait { dur: pv(&raw), raw, remaining: 0 }
            }
            "emit_signal" => K::Instant(Leaf::Emit(pv(&signal_name(args, params)))),
            "emit_global_signal" => K::Instant(Leaf::EmitGlobal(pv(&signal_name(args, params)))),
            "wait_for_signal" => K::WaitSignal { sig: pv(&signal_name(args, params)), cur: None },
            "emit_signal_to" => {
                let (npc, sig) = (arg(0, "npc", None)?, arg(1, "signal", None)?);
                K::Instant(Leaf::EmitTo(pv(&npc), pv(&sig)))
            }
            "succeed" => K::Instant(Leaf::Succeed),
            "fail" => K::Instant(Leaf::Fail),
            "repeat" => {
                let count = arg(0, "count", Some("1"))?;
                if !params.is_some_and(Key::has_subs) {
                    return None;
                }
                K::Repeat { count: pv(&count), remaining: 0 }
            }
            "set" => {
                let (key, value) = (arg(0, "key", None)?, arg(1, "value", None)?);
                K::Instant(Leaf::Set(key, pv(&value)))
            }
            "wait_until" => K::WaitUntil(pv(&arg(0, "condition", None)?)),
            "cooldown" => {
                let key = arg(0, "key", None)?;
                let dur = arg(1, "duration", Some("1s"))?;
                K::Instant(Leaf::Cooldown(format!("cooldown.{key}"), pv(&dur)))
            }
            "forget" => K::Instant(Leaf::Forget(arg(0, "key", None)?)),
            "clear_memory" => K::Instant(Leaf::ClearMemory),
            "walkto" => {
                let (mut x, mut y, mut z, mut speed) = (None, None, None, None);
                if args.len() >= 3 {
                    x = Some(args[0].clone());
                    y = Some(args[1].clone());
                    z = Some(args[2].clone());
                    for a in &args[3..] {
                        if let Some((k, v)) = a.split_once('=')
                            && k.eq_ignore_ascii_case("speed")
                        {
                            speed = Some(v.to_owned());
                        }
                    }
                } else if let Some(p) = params {
                    x = p.string("x");
                    y = p.string("y");
                    z = p.string("z");
                    speed = p.string("speed");
                }
                K::Walk { x: pv(&x?), y: pv(&y?), z: pv(&z?), speed: speed.map(|s| pv(&s)), started: false }
            }
            "wander" => {
                let m = parse_args(args, 1, params);
                let radius = m.get("0").or_else(|| m.get("radius")).cloned().unwrap_or_else(|| "10".to_owned());
                K::Wander { radius: pv(&radius), started: false }
            }
            "break_block" => {
                let m = parse_args(args, 3, params);
                let get = |a: &str, b: &str| m.get(a).or_else(|| m.get(b)).cloned();
                let (x, y, z) = (get("0", "x")?, get("1", "y")?, get("2", "z")?);
                let radius = get("3", "radius").unwrap_or_else(|| "3".to_owned());
                K::BreakBlock { x: pv(&x), y: pv(&y), z: pv(&z), radius: pv(&radius), waited: 0, total: 0 }
            }
            "print" | "say" => {
                // Unquoted words are one message here; the original takes the first word only.
                let m = if args.is_empty() {
                    arg(0, "message", None).or_else(|| params.and_then(|p| p.value.clone()))?
                } else {
                    args.join(" ")
                };
                let leaf = if name.eq_ignore_ascii_case("say") { Leaf::Say(pv(&m)) } else { Leaf::Print(pv(&m)) };
                K::Instant(leaf)
            }
            "look" => {
                let (x, y, z) = (arg(0, "x", None)?, arg(1, "y", None)?, arg(2, "z", None)?);
                K::Instant(Leaf::Look(pv(&x), pv(&y), pv(&z)))
            }
            "teleport" => {
                let (x, y, z) = (arg(0, "x", None)?, arg(1, "y", None)?, arg(2, "z", None)?);
                let world = arg(3, "world", None);
                K::Instant(Leaf::Teleport(pv(&x), pv(&y), pv(&z), world.map(|w| pv(&w))))
            }
            _ => return None,
        };
        Some(self.add(k))
    }
}

fn signal_name(args: &[String], params: Option<&Key>) -> String {
    args.first().cloned().or_else(|| params.and_then(|p| p.value.clone())).unwrap_or_default()
}

fn arg_or_param(args: &[String], idx: usize, name: &str, params: Option<&Key>, default: Option<&str>) -> Option<String> {
    if let Some(a) = args.get(idx) {
        return Some(a.clone());
    }
    if let Some(p) = params {
        return p.string(name).or_else(|| default.map(str::to_owned));
    }
    default.map(str::to_owned)
}

/// `BehaviorContext.parseArgs`: the first `positional` arguments by position, then `key=value` pairs; a
/// behavior with fewer arguments reads its key's sub keys.
fn parse_args(args: &[String], positional: usize, params: Option<&Key>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if args.len() >= positional && !args.is_empty() {
        for (i, a) in args.iter().take(positional).enumerate() {
            m.insert(i.to_string(), a.clone());
        }
        for a in args.iter().skip(positional) {
            if let Some((k, v)) = a.split_once('=') {
                m.insert(k.to_ascii_lowercase(), v.to_owned());
            }
        }
    } else if let Some(p) = params {
        for s in &p.subs {
            if let Some(v) = &s.value {
                m.insert(s.name.clone(), v.clone());
            }
        }
    }
    m
}

/// `parseInlineArgs`: words, with `'...'`, `"..."` and `` `...` `` quoting and backslash escapes; a backtick
/// quote keeps its backticks.
pub fn inline_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in input.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if quote.is_none() && (c == '"' || c == '\'' || c == '`') {
            quote = Some(c);
            if c == '`' {
                cur.push(c);
            }
            continue;
        }
        if quote == Some(c) {
            quote = None;
            if c == '`' {
                cur.push(c);
            }
            continue;
        }
        if quote.is_none() && c.is_whitespace() {
            if !cur.is_empty() {
                args.push(std::mem::take(&mut cur));
            }
            continue;
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args
}

impl Tree {
    /// `BehaviorTreeParser.parse`: the tree under `key`; `None` when it has nothing to run.
    pub fn parse(key: &Key) -> Option<Tree> {
        let mut p = Parser { nodes: Vec::new() };
        let root = p.node(key)?;
        Some(Tree { nodes: p.nodes, root })
    }

    /// The root of a `traits.behavior.tree` map: its first key is the tree (`getSubKeys().iterator().next()`).
    pub fn from_tree_node(tree: &Node) -> Option<Tree> {
        let first = tree.entries().first()?;
        Tree::parse(&Key::from_node(&first.key, &first.value))
    }

    /// `RootBehavior.shouldExecute`.
    pub fn should_execute(&mut self, cx: &mut Cx) -> bool {
        let r = self.root;
        self.should(r, cx)
    }

    /// `RootBehavior.run`: the root runs; one that finished starts over the next time.
    pub fn run_root(&mut self, cx: &mut Cx) -> Status {
        let r = self.root;
        let s = self.run(r, cx);
        if matches!(s, Status::Success | Status::Failure) {
            self.reset(r, cx);
        }
        s
    }

    /// `Behavior.reset` of the root (the NPC despawned).
    pub fn reset_root(&mut self, cx: &mut Cx) {
        let r = self.root;
        self.reset(r, cx);
    }

    fn comp(&self, n: usize) -> Comp {
        match &self.nodes[n] {
            K::Sequence { c, .. } | K::Selector { c, .. } | K::Parallel { c, .. } => c.clone(),
            _ => Comp::default(),
        }
    }

    fn put_comp(&mut self, n: usize, comp: Comp) {
        match &mut self.nodes[n] {
            K::Sequence { c, .. } | K::Selector { c, .. } | K::Parallel { c, .. } => *c = comp,
            _ => {}
        }
    }

    /// `Composite.tickParallel`.
    fn tick_parallel(&mut self, n: usize, cx: &mut Cx) {
        let mut comp = self.comp(n);
        for b in comp.parallel.clone() {
            if self.should(b, cx) && !comp.executing.contains(&b) {
                comp.executing.push(b);
            }
        }
        let mut i = 0;
        while i < comp.executing.len() {
            let b = comp.executing[i];
            let status = self.run(b, cx);
            match status {
                Status::ResetAndRemove => {
                    comp.behaviors.retain(|x| *x != b);
                    comp.parallel.retain(|x| *x != b);
                    comp.executing.remove(i);
                    self.reset(b, cx);
                }
                Status::Failure | Status::Success => {
                    comp.executing.remove(i);
                    self.reset(b, cx);
                }
                Status::Running => i += 1,
            }
        }
        self.put_comp(n, comp);
    }

    /// `Composite.reset`.
    fn reset_comp(&mut self, n: usize, cx: &mut Cx) {
        let comp = self.comp(n);
        for b in comp.executing.clone() {
            self.reset(b, cx);
        }
        let mut comp = comp;
        comp.executing.clear();
        self.put_comp(n, comp);
    }

    pub fn reset(&mut self, n: usize, cx: &mut Cx) {
        match self.nodes[n].clone() {
            K::Empty | K::Instant(_) | K::EvalBlock(_) | K::WaitUntil(_) => {}
            K::Wait { raw, dur, .. } => {
                let ticks = dur.as_string(cx.sc);
                let t = expr::parse_ticks(&ticks);
                let _ = raw;
                if let K::Wait { remaining, .. } = &mut self.nodes[n] {
                    *remaining = t;
                }
            }
            K::WaitSignal { cur, .. } => {
                if let Some((sig, token)) = cur {
                    cx.sig.unlisten(cx.npc, &sig, token);
                }
                if let K::WaitSignal { cur, .. } = &mut self.nodes[n] {
                    *cur = None;
                }
            }
            K::Repeat { count, .. } => {
                let total = count.as_number(cx.sc) as i64;
                if let K::Repeat { remaining, .. } = &mut self.nodes[n] {
                    *remaining = total;
                }
            }
            K::Walk { .. } => {
                if let K::Walk { started, .. } = &mut self.nodes[n] {
                    *started = false;
                }
            }
            K::Wander { .. } => {
                if let K::Wander { started, .. } = &mut self.nodes[n] {
                    *started = false;
                }
            }
            K::BreakBlock { .. } => {
                if let K::BreakBlock { waited, total, .. } = &mut self.nodes[n] {
                    *waited = 0;
                    *total = 0;
                }
            }
            K::Sequence { executing, .. } => {
                self.reset_comp(n, cx);
                if let Some(e) = executing {
                    self.reset(e, cx);
                }
                if let K::Sequence { executing, index, .. } = &mut self.nodes[n] {
                    *executing = None;
                    *index = -1;
                }
            }
            K::Selector { executing, .. } => {
                self.reset_comp(n, cx);
                if let Some(e) = executing {
                    self.reset(e, cx);
                }
                if let K::Selector { executing, .. } = &mut self.nodes[n] {
                    *executing = None;
                }
            }
            K::Parallel { .. } => {
                self.reset_comp(n, cx);
                if let K::Parallel { started, .. } = &mut self.nodes[n] {
                    *started = false;
                }
            }
            K::Timeout { child, .. } => {
                self.reset(child, cx);
                if let K::Timeout { elapsed, .. } = &mut self.nodes[n] {
                    *elapsed = 0;
                }
            }
            K::Invert { child } | K::ParallelWrap { child } | K::Loop { child, .. } => self.reset(child, cx),
            K::IfElse { executing, .. } => {
                if let Some(e) = executing {
                    self.reset(e, cx);
                }
                if let K::IfElse { executing, .. } = &mut self.nodes[n] {
                    *executing = None;
                }
            }
            K::Coalesced { children } => {
                for c in children {
                    self.reset(c, cx);
                }
            }
        }
    }

    fn should(&mut self, n: usize, cx: &mut Cx) -> bool {
        match self.nodes[n].clone() {
            K::Empty | K::Instant(_) | K::EvalBlock(_) | K::WaitSignal { .. } | K::WaitUntil(_) | K::Parallel { .. } => {
                if let K::WaitSignal { sig, .. } = &self.nodes[n].clone() {
                    let name = sig.as_string(cx.sc);
                    let token = cx.sig.listen(cx.npc, &name);
                    if let K::WaitSignal { cur, .. } = &mut self.nodes[n] {
                        *cur = Some((name, token));
                    }
                }
                true
            }
            K::Wait { raw, dur, .. } => {
                let t = expr::parse_ticks(&raw);
                let ticks = if t >= 0 { t } else { dur.as_number(cx.sc) as i64 };
                if let K::Wait { remaining, .. } = &mut self.nodes[n] {
                    *remaining = ticks;
                }
                ticks > 0
            }
            K::Repeat { count, .. } => {
                let total = count.as_number(cx.sc) as i64;
                if let K::Repeat { remaining, .. } = &mut self.nodes[n] {
                    *remaining = total;
                }
                total > 0
            }
            K::Walk { .. } => cx.env.spawned() && !cx.env.navigating(),
            K::Wander { .. } | K::BreakBlock { .. } => cx.env.spawned(),
            K::Sequence { c, .. } | K::Selector { c, .. } => !c.behaviors.is_empty(),
            K::Timeout { child, .. } | K::Invert { child } | K::ParallelWrap { child } | K::Loop { child, .. } => self.should(child, cx),
            K::IfElse { cond, then, els, .. } => {
                let pick = if eval_cond(&cond, cx.sc) { Some(then) } else { els };
                let ok = match pick {
                    Some(p) => self.should(p, cx),
                    None => false,
                };
                if let K::IfElse { executing, .. } = &mut self.nodes[n] {
                    *executing = if ok { pick } else { None };
                }
                ok
            }
            K::Coalesced { children } => {
                for c in children {
                    if self.should(c, cx) {
                        return true;
                    }
                }
                false
            }
        }
    }

    pub fn run(&mut self, n: usize, cx: &mut Cx) -> Status {
        match self.nodes[n].clone() {
            K::Empty => Status::Success,
            K::Instant(leaf) => self.run_leaf(&leaf, cx),
            K::EvalBlock(list) => {
                for e in &list {
                    e.eval(cx.sc);
                }
                Status::Success
            }
            K::Wait { .. } => {
                if let K::Wait { remaining, .. } = &mut self.nodes[n] {
                    *remaining -= 1;
                    if *remaining <= 0 {
                        return Status::Success;
                    }
                }
                Status::Running
            }
            K::WaitSignal { cur, .. } => {
                let fired = cur.as_ref().is_some_and(|(_, t)| cx.sig.was_fired(*t));
                if fired && let Some((sig, token)) = cur {
                    cx.sig.unlisten(cx.npc, &sig, token);
                    if let K::WaitSignal { cur, .. } = &mut self.nodes[n] {
                        *cur = None;
                    }
                    return Status::Success;
                }
                Status::Running
            }
            K::Repeat { .. } => {
                if let K::Repeat { remaining, .. } = &mut self.nodes[n] {
                    if *remaining <= 0 {
                        return Status::Success;
                    }
                    *remaining -= 1;
                }
                Status::Running
            }
            K::WaitUntil(cond) => {
                if cond.as_bool(cx.sc) { Status::Success } else { Status::Running }
            }
            K::Walk { x, y, z, speed, started } => {
                if !started {
                    let p = [x.as_number(cx.sc), y.as_number(cx.sc), z.as_number(cx.sc)];
                    let s = speed.as_ref().map(|s| s.as_number(cx.sc));
                    cx.env.walk_to(p, s);
                    if let K::Walk { started, .. } = &mut self.nodes[n] {
                        *started = true;
                    }
                }
                if cx.env.navigating() { Status::Running } else { Status::Success }
            }
            K::Wander { radius, started } => {
                if !cx.env.spawned() {
                    return Status::Failure;
                }
                if !started {
                    let r = radius.as_number(cx.sc) as i64;
                    if !cx.env.wander(r) {
                        return Status::Success;
                    }
                    if let K::Wander { started, .. } = &mut self.nodes[n] {
                        *started = true;
                    }
                }
                if cx.env.navigating() { Status::Running } else { Status::Success }
            }
            K::BreakBlock { x, y, z, radius, .. } => {
                if !cx.env.spawned() {
                    return Status::Failure;
                }
                let p = [x.as_number(cx.sc).floor() as i64, y.as_number(cx.sc).floor() as i64, z.as_number(cx.sc).floor() as i64];
                let r = radius.as_number(cx.sc).max(1.0);
                let centre = [p[0] as f64 + 0.5, p[1] as f64, p[2] as f64 + 0.5];
                let (mut waited, mut total) = (0, 0);
                if let K::BreakBlock { waited: w, total: t, .. } = &mut self.nodes[n] {
                    *t += 1;
                    total = *t;
                    waited = *w;
                }
                if total > 600 {
                    return Status::Failure;
                }
                if cx.env.distance_to(centre) > r {
                    if !cx.env.navigating() {
                        cx.env.walk_to(centre, None);
                    }
                    return Status::Running;
                }
                waited += 1;
                if let K::BreakBlock { waited: w, .. } = &mut self.nodes[n] {
                    *w = waited;
                }
                if waited >= 20 {
                    cx.env.break_block(p);
                    return Status::Success;
                }
                Status::Running
            }
            K::Sequence { .. } => self.run_sequence(n, cx),
            K::Selector { .. } => self.run_selector(n, cx),
            K::Parallel { started, .. } => self.run_parallel(n, started, cx),
            K::Timeout { child, max, elapsed } => {
                if elapsed >= max {
                    self.reset(child, cx);
                    return Status::Failure;
                }
                let s = self.run(child, cx);
                if let K::Timeout { elapsed: e, .. } = &mut self.nodes[n] {
                    *e = if s != Status::Running { 0 } else { elapsed + 1 };
                }
                s
            }
            K::Invert { child } => match self.run(child, cx) {
                Status::Success => Status::Failure,
                Status::Failure => Status::Success,
                other => other,
            },
            K::Loop { child, cond } => {
                let s = self.run(child, cx);
                if s == Status::Success {
                    self.reset(child, cx);
                    if eval_cond(&cond, cx.sc) && self.should(child, cx) {
                        return Status::Running;
                    }
                }
                s
            }
            K::IfElse { executing, .. } => match executing {
                Some(e) => self.run(e, cx),
                None => Status::Failure,
            },
            K::Coalesced { children } => {
                for c in children {
                    let s = self.run(c, cx);
                    if matches!(s, Status::Failure | Status::ResetAndRemove) {
                        return s;
                    }
                }
                Status::Success
            }
            K::ParallelWrap { child } => self.run(child, cx),
        }
    }

    /// `ParallelComposite`: every child starts together and runs until it finishes; the container finishes with the last.
    fn run_parallel(&mut self, n: usize, started: bool, cx: &mut Cx) -> Status {
        let mut comp = self.comp(n);
        if !started {
            for b in comp.parallel.clone() {
                if self.should(b, cx) && !comp.executing.contains(&b) {
                    comp.executing.push(b);
                }
            }
            if let K::Parallel { started, .. } = &mut self.nodes[n] {
                *started = true;
            }
        }
        let mut i = 0;
        while i < comp.executing.len() {
            let b = comp.executing[i];
            match self.run(b, cx) {
                Status::Running => i += 1,
                _ => {
                    comp.executing.remove(i);
                    self.reset(b, cx);
                }
            }
        }
        let empty = comp.executing.is_empty();
        self.put_comp(n, comp);
        if empty {
            if let K::Parallel { started, .. } = &mut self.nodes[n] {
                *started = false;
            }
            Status::Success
        } else {
            Status::Running
        }
    }

    fn run_sequence(&mut self, n: usize, cx: &mut Cx) -> Status {
        self.tick_parallel(n, cx);
        let (mut executing, mut index) = match &self.nodes[n] {
            K::Sequence { executing, index, .. } => (*executing, *index),
            _ => (None, -1),
        };
        let mut comp = self.comp(n);
        let result;
        if executing.is_none() {
            let next = self.select_next(&comp, &mut executing, &mut index, cx);
            if next != Status::Running {
                self.reset_current(&mut executing, cx);
                self.store_seq(n, comp, executing, index);
                return next;
            }
        }
        let Some(ex) = executing else { return Status::Failure };
        let status = self.run(ex, cx);
        match status {
            Status::Failure => {
                self.reset_current(&mut executing, cx);
                result = Status::Failure;
            }
            Status::ResetAndRemove => {
                if index >= 0 && (index as usize) < comp.behaviors.len() {
                    comp.behaviors.remove(index as usize);
                }
                index -= 1;
                result = self.select_next(&comp, &mut executing, &mut index, cx);
            }
            Status::Success => {
                self.reset_current(&mut executing, cx);
                result = self.select_next(&comp, &mut executing, &mut index, cx);
            }
            Status::Running => result = status,
        }
        self.store_seq(n, comp, executing, index);
        result
    }

    fn store_seq(&mut self, n: usize, comp: Comp, executing: Option<usize>, index: i64) {
        if let K::Sequence { c, executing: e, index: i } = &mut self.nodes[n] {
            // Keep the parallel bookkeeping `tick_parallel` just wrote.
            let running = c.executing.clone();
            *c = comp;
            c.executing = running;
            *e = executing;
            *i = index;
        }
    }

    fn reset_current(&mut self, executing: &mut Option<usize>, cx: &mut Cx) {
        if let Some(e) = executing.take() {
            self.reset(e, cx);
        }
    }

    /// `Sequence.selectNext`.
    fn select_next(&mut self, comp: &Comp, executing: &mut Option<usize>, index: &mut i64, cx: &mut Cx) -> Status {
        *index += 1;
        if *index < 0 || (*index as usize) >= comp.behaviors.len() {
            return Status::Success;
        }
        let b = comp.behaviors[*index as usize];
        *executing = Some(b);
        if !self.should(b, cx) {
            self.reset_current(executing, cx);
            return Status::Failure;
        }
        Status::Running
    }

    fn run_selector(&mut self, n: usize, cx: &mut Cx) -> Status {
        self.tick_parallel(n, cx);
        let mut executing = match &self.nodes[n] {
            K::Selector { executing, .. } => *executing,
            _ => None,
        };
        let mut status = None;
        if executing.is_none() {
            let comp = self.comp(n);
            if comp.behaviors.is_empty() {
                return Status::Failure;
            }
            let pick = ((cx.env.random() * comp.behaviors.len() as f64) as usize).min(comp.behaviors.len() - 1);
            let b = comp.behaviors[pick];
            executing = Some(b);
            if !self.should(b, cx) {
                status = Some(Status::Failure);
            }
        }
        let Some(ex) = executing else { return Status::Failure };
        let status = match status {
            Some(s) => s,
            None => self.run(ex, cx),
        };
        let out = match status {
            Status::ResetAndRemove => {
                let mut comp = self.comp(n);
                comp.behaviors.retain(|x| *x != ex);
                self.put_comp(n, comp);
                self.reset(ex, cx);
                executing = None;
                Status::Success
            }
            Status::Success | Status::Failure => {
                self.reset(ex, cx);
                executing = None;
                status
            }
            Status::Running => status,
        };
        if let K::Selector { executing: e, .. } = &mut self.nodes[n] {
            *e = executing;
        }
        out
    }

    fn run_leaf(&mut self, leaf: &Leaf, cx: &mut Cx) -> Status {
        match leaf {
            Leaf::Succeed => Status::Success,
            Leaf::Fail => Status::Failure,
            Leaf::Command(c) => {
                let line = c.as_string(cx.sc);
                let (op, player) = (line.contains("-o"), line.contains("-p"));
                cx.env.command(&line, op, player);
                Status::Success
            }
            Leaf::Expr(e) => {
                e.evaluate(cx.sc);
                Status::Success
            }
            Leaf::Say(m) => {
                let msg = m.as_string(cx.sc);
                cx.env.say(&msg);
                Status::Success
            }
            Leaf::Print(m) => {
                let msg = m.as_string(cx.sc);
                cx.env.log(&msg);
                Status::Success
            }
            Leaf::Look(x, y, z) => {
                if !cx.env.spawned() {
                    return Status::Success;
                }
                let p = [x.as_number(cx.sc), y.as_number(cx.sc), z.as_number(cx.sc)];
                cx.env.look_at(p);
                Status::Success
            }
            Leaf::Teleport(x, y, z, w) => {
                if !cx.env.spawned() {
                    return Status::Success;
                }
                let p = [x.as_number(cx.sc), y.as_number(cx.sc), z.as_number(cx.sc)];
                let world = w.as_ref().map(|w| w.as_string(cx.sc));
                cx.env.teleport(world.as_deref(), p);
                Status::Success
            }
            Leaf::Emit(s) => {
                let name = s.as_string(cx.sc);
                cx.sig.emit_to(cx.npc, &name);
                Status::Success
            }
            Leaf::EmitGlobal(s) => {
                let name = s.as_string(cx.sc);
                cx.sig.emit_global(&name);
                Status::Success
            }
            Leaf::EmitTo(who, s) => {
                let target = match who.evaluate(cx.sc) {
                    Value::Num(n) => cx.env.find_npc(&format!("{}", n as i64)),
                    other => cx.env.find_npc(&other.text()),
                };
                let Some(t) = target else { return Status::Failure };
                let name = s.as_string(cx.sc);
                cx.sig.emit_to(t, &name);
                Status::Success
            }
            Leaf::Set(key, v) => {
                let value = v.evaluate(cx.sc);
                cx.sc.memory.set(key, value);
                Status::Success
            }
            Leaf::Cooldown(key, dur) => {
                let last = cx.sc.memory.get_number(key, 0.0) as i64;
                let ms = expr::parse_millis(&dur.as_string(cx.sc));
                let now = cx.env.now_ms() as i64;
                if now - last >= ms {
                    cx.sc.memory.set(key, Value::Num(now as f64));
                    Status::Success
                } else {
                    Status::Failure
                }
            }
            Leaf::Forget(k) => {
                cx.sc.memory.remove(k);
                Status::Success
            }
            Leaf::ClearMemory => {
                cx.sc.memory.clear();
                Status::Success
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_citizens_core::yaml;

    #[derive(Default)]
    struct Fake {
        spawned: bool,
        nav: bool,
        walks: Vec<([f64; 3], Option<f64>)>,
        wanders: Vec<i64>,
        looks: Vec<[f64; 3]>,
        tps: Vec<(Option<String>, [f64; 3])>,
        cmds: Vec<(String, bool, bool)>,
        says: Vec<String>,
        logs: Vec<String>,
        breaks: Vec<[i64; 3]>,
        pos: [f64; 3],
        now: u64,
        r: f64,
    }

    impl Env for Fake {
        fn spawned(&self) -> bool {
            self.spawned
        }
        fn navigating(&self) -> bool {
            self.nav
        }
        fn walk_to(&mut self, t: [f64; 3], s: Option<f64>) {
            self.walks.push((t, s));
            self.nav = true;
        }
        fn wander(&mut self, r: i64) -> bool {
            self.wanders.push(r);
            self.nav = true;
            true
        }
        fn look_at(&mut self, p: [f64; 3]) {
            self.looks.push(p);
        }
        fn teleport(&mut self, w: Option<&str>, p: [f64; 3]) {
            self.tps.push((w.map(str::to_owned), p));
        }
        fn command(&mut self, l: &str, op: bool, player: bool) {
            self.cmds.push((l.to_owned(), op, player));
        }
        fn say(&mut self, m: &str) {
            self.says.push(m.to_owned());
        }
        fn log(&mut self, m: &str) {
            self.logs.push(m.to_owned());
        }
        fn distance_to(&self, p: [f64; 3]) -> f64 {
            ((p[0] - self.pos[0]).powi(2) + (p[1] - self.pos[1]).powi(2) + (p[2] - self.pos[2]).powi(2)).sqrt()
        }
        fn break_block(&mut self, p: [i64; 3]) {
            self.breaks.push(p);
        }
        fn find_npc(&self, s: &str) -> Option<i32> {
            s.parse().ok()
        }
        fn now_ms(&self) -> u64 {
            self.now
        }
        fn random(&mut self) -> f64 {
            self.r
        }
    }

    fn y(lines: &[&str]) -> String {
        lines.join("\n") + "\n"
    }

    struct Rig {
        tree: Tree,
        env: Fake,
        sc: Scope,
        sig: Signals,
    }

    impl Rig {
        fn new(text: &str) -> Rig {
            let doc = yaml::parse(text).expect("yaml");
            let tree = Tree::from_tree_node(&doc).expect("a tree");
            Rig { tree, env: Fake { spawned: true, ..Fake::default() }, sc: Scope::new(), sig: Signals::default() }
        }

        /// One controller tick: start when idle, run, and say whether it keeps running.
        fn tick(&mut self, running: &mut bool) -> Status {
            let mut cx = Cx { env: &mut self.env, sc: &mut self.sc, sig: &mut self.sig, npc: 1 };
            if !*running && !self.tree.should_execute(&mut cx) {
                return Status::Failure;
            }
            let s = self.tree.run_root(&mut cx);
            *running = s == Status::Running;
            s
        }

        fn run_to_end(&mut self, limit: usize) -> Status {
            let mut running = false;
            let mut last = Status::Failure;
            for _ in 0..limit {
                last = self.tick(&mut running);
                if last != Status::Running {
                    break;
                }
            }
            last
        }
    }

    #[test]
    fn a_sequence_runs_its_children_in_order() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': say hello", "  '1': print there", "  '2': /time set day"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["hello"]);
        assert_eq!(r.env.logs, vec!["there"]);
        assert_eq!(r.env.cmds, vec![("time set day".to_owned(), false, false)]);
    }

    #[test]
    fn a_list_reads_as_numbered_children() {
        let mut r = Rig::new(&y(&["sequence:", "  - say a", "  - say b"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["a", "b"]);
    }

    #[test]
    fn wait_counts_ticks() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': wait 5", "  '1': say done"]));
        let mut running = false;
        for _ in 0..5 {
            assert_eq!(r.tick(&mut running), Status::Running);
            assert!(r.env.says.is_empty());
        }
        assert_eq!(r.tick(&mut running), Status::Success);
        assert_eq!(r.env.says, vec!["done"]);
    }

    #[test]
    fn a_wait_takes_seconds_too() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': wait 1s", "  '1': say done"]));
        let mut running = false;
        let mut ticks = 0;
        loop {
            ticks += 1;
            if r.tick(&mut running) != Status::Running {
                break;
            }
            assert!(ticks < 100);
        }
        assert_eq!(ticks, 21, "twenty ticks of waiting, then the next child runs");
    }

    #[test]
    fn a_failing_child_fails_the_sequence() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': say a", "  '1': fail", "  '2': say b"]));
        assert_eq!(r.run_to_end(5), Status::Failure);
        assert_eq!(r.env.says, vec!["a"]);
    }

    #[test]
    fn invert_swaps_success_and_failure() {
        let mut r = Rig::new(&y(&["invert:", "  '0': fail"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        let mut r = Rig::new(&y(&["invert:", "  '0': succeed"]));
        assert_eq!(r.run_to_end(5), Status::Failure);
    }

    #[test]
    fn set_cooldown_and_forget_use_the_memory() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': set hp 7", "  '1': cooldown c 10s", "  '2': forget hp"]));
        r.env.now = 100_000;
        assert_eq!(r.run_to_end(5), Status::Success);
        assert!(!r.sc.memory.has("hp"));
        assert_eq!(r.sc.memory.get_number("cooldown.c", 0.0), 100_000.0);
        // Within ten seconds the cooldown refuses.
        r.env.now = 105_000;
        assert_eq!(r.run_to_end(5), Status::Failure);
        r.env.now = 110_000;
        assert_eq!(r.run_to_end(5), Status::Success);
    }

    #[test]
    fn set_stores_an_evaluated_expression() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': set n `2 * 3 + 1`"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.sc.memory.get_number("n", 0.0), 7.0);
    }

    #[test]
    fn clear_memory_empties_it() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': set a 1", "  '1': set b 2", "  '2': clear_memory"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.sc.memory.size(), 0);
    }

    #[test]
    fn if_else_picks_by_the_condition() {
        let mut r = Rig::new(&y(&["sequence:", "  '0':", "    if `v.x = 1; v.x > 0`: say yes", "  '1':", "    else: say no"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["yes"]);
        let mut r = Rig::new(&y(&["sequence:", "  '0':", "    if false: say yes", "  '1':", "    else: say no"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["no"]);
    }

    #[test]
    fn an_if_without_else_keeps_the_next_sibling() {
        let mut r = Rig::new(&y(&["sequence:", "  '0':", "    if true: say first", "  '1': say second"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["first", "second"]);
    }

    #[test]
    fn a_loop_repeats_while_its_condition_holds() {
        let mut r = Rig::new(&y(&[
            "sequence:",
            "  '0': set n 0",
            "  '1':",
            "    \"loop `mem.get('n') < 3`\":",
            "      '0': \"`mem.set('n', mem.get('n') + 1)`\"",
            "  '2': say out",
        ]));
        assert_eq!(r.run_to_end(50), Status::Success);
        assert_eq!(r.sc.memory.get_number("n", 0.0), 3.0);
        assert_eq!(r.env.says, vec!["out"]);
    }

    #[test]
    fn a_timeout_fails_a_child_that_runs_too_long() {
        let mut r = Rig::new(&y(&["timeout 3:", "  '0': wait 100"]));
        let mut running = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.tick(&mut running), Status::Failure);
    }

    #[test]
    fn a_selector_runs_one_child() {
        let mut r = Rig::new(&y(&["random:", "  '0': say a", "  '1': say b"]));
        r.env.r = 0.99;
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["b"]);
    }

    #[test]
    fn parallel_starts_all_children_at_once_and_ends_with_the_last() {
        let mut r = Rig::new(&y(&["parallel:", "  '0': wait 2", "  '1': wait 4"]));
        let mut running = false;
        let mut ticks = 0;
        loop {
            ticks += 1;
            if r.tick(&mut running) != Status::Running {
                break;
            }
            assert!(ticks < 20);
        }
        assert_eq!(ticks, 4, "the longer wait decides");
        let mut r = Rig::new(&y(&["parallel:", "  '0': say a", "  '1': say b"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["a", "b"]);
    }

    #[test]
    fn eval_runs_each_child_as_an_expression() {
        let mut r = Rig::new(&y(&["eval:", "  '0': mem.set('a', 4)", "  '1': mem.set('b', mem.get('a') * 2)"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.sc.memory.get_number("b", 0.0), 8.0);
    }

    #[test]
    fn a_backtick_line_is_an_expression_behavior() {
        let mut r = Rig::new(&y(&["sequence:", "  - \"`mem.set('k', 5)`\"", "  - say ok"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.sc.memory.get_number("k", 0.0), 5.0);
    }

    #[test]
    fn walkto_follows_the_navigator() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': walkto 10 64 -3 speed=1.5", "  '1': say there"]));
        let mut running = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.env.walks, vec![([10.0, 64.0, -3.0], Some(1.5))]);
        assert!(r.env.says.is_empty());
        r.env.nav = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.tick(&mut running), Status::Success);
        assert_eq!(r.env.says, vec!["there"]);
    }

    #[test]
    fn walkto_reads_named_parameters() {
        let mut r = Rig::new(&y(&["sequence:", "  '0':", "    walkto:", "      x: 1", "      y: 2", "      z: 3", "      speed: 2"]));
        let mut running = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.env.walks, vec![([1.0, 2.0, 3.0], Some(2.0))]);
    }

    #[test]
    fn look_and_teleport_reach_the_env() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': look 1 2 3", "  '1': teleport 4 5 6 nether"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.looks, vec![[1.0, 2.0, 3.0]]);
        assert_eq!(r.env.tps, vec![(Some("nether".to_owned()), [4.0, 5.0, 6.0])]);
    }

    #[test]
    fn wander_starts_a_walk_and_waits_for_it() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': wander 6"]));
        let mut running = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.env.wanders, vec![6]);
        r.env.nav = false;
        assert_eq!(r.tick(&mut running), Status::Success);
    }

    #[test]
    fn break_block_walks_up_then_breaks() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': break_block 10 64 10 3"]));
        let mut running = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.env.walks.len(), 1);
        r.env.pos = [10.0, 64.0, 10.0];
        let mut last = Status::Running;
        for _ in 0..40 {
            last = r.tick(&mut running);
            if last != Status::Running {
                break;
            }
        }
        assert_eq!(last, Status::Success);
        assert_eq!(r.env.breaks, vec![[10, 64, 10]]);
    }

    #[test]
    fn signals_wake_a_waiting_behavior() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': wait_for_signal go", "  '1': say went"]));
        let mut running = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.tick(&mut running), Status::Running);
        r.sig.emit_to(1, "go");
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.tick(&mut running), Status::Success);
        assert_eq!(r.env.says, vec!["went"]);
    }

    #[test]
    fn a_global_signal_reaches_other_npcs() {
        let mut s = Signals::default();
        let a = s.listen(1, "x");
        let b = s.listen(2, "x");
        s.emit_global("x");
        assert!(s.was_fired(a) && s.was_fired(b));
        s.unlisten(1, "x", a);
        assert!(!s.was_fired(a));
        s.emit_to(2, "y");
        s.clear(2);
        assert!(!s.was_fired(b));
    }

    #[test]
    fn emit_signal_to_reaches_the_named_npc() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': emit_signal_to 7 ping"]));
        let t = r.sig.listen(7, "ping");
        assert_eq!(r.run_to_end(5), Status::Success);
        assert!(r.sig.was_fired(t));
    }

    #[test]
    fn wait_until_runs_until_its_condition_holds() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': wait_until \"`mem.get('ready') > 0`\"", "  '1': say go"]));
        let mut running = false;
        assert_eq!(r.tick(&mut running), Status::Running);
        r.sc.memory.set("ready", Value::Num(1.0));
        assert_eq!(r.tick(&mut running), Status::Running);
        assert_eq!(r.tick(&mut running), Status::Success);
        assert_eq!(r.env.says, vec!["go"]);
    }

    #[test]
    fn an_unknown_behavior_is_an_empty_one() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': nosuchthing", "  '1': say after"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.says, vec!["after"]);
    }

    #[test]
    fn inline_arguments_keep_quotes_and_backticks() {
        assert_eq!(inline_args("a b  c"), vec!["a", "b", "c"]);
        assert_eq!(inline_args("say \"hello world\" k='v w'"), vec!["say", "hello world", "k=v w"]);
        assert_eq!(inline_args("set n `1 + 2`"), vec!["set", "n", "`1 + 2`"]);
        assert_eq!(inline_args("a\\ b"), vec!["a b"]);
    }

    #[test]
    fn command_flags_are_read_from_the_line() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': /say hi -o -p"]));
        assert_eq!(r.run_to_end(5), Status::Success);
        assert_eq!(r.env.cmds, vec![("say hi -o -p".to_owned(), true, true)]);
    }

    #[test]
    fn repeat_counts_then_succeeds() {
        // `repeat` needs a key with sub keys; its count is read from the first argument.
        let mut r = Rig::new(&y(&["sequence:", "  '0':", "    repeat 2:", "      x: 1"]));
        let mut running = false;
        let mut n = 0;
        loop {
            n += 1;
            if r.tick(&mut running) != Status::Running || n > 10 {
                break;
            }
        }
        assert!(n <= 10);
    }

    #[test]
    fn the_root_starts_over_after_finishing() {
        let mut r = Rig::new(&y(&["sequence:", "  '0': say tick"]));
        let mut running = false;
        r.tick(&mut running);
        r.tick(&mut running);
        assert_eq!(r.env.says, vec!["tick", "tick"]);
    }

    #[test]
    fn a_tree_without_keys_is_none() {
        assert!(Tree::from_tree_node(&Node::map()).is_none());
    }
}
