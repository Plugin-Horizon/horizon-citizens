//! Citizens' expression registry (`ExpressionRegistry`, `ExpressionScope`, `Memory`) and its one engine,
//! Molang (`MolangEngine`, which the original runs on the Mocha library).
//!
//! Citizens registers the Molang engine only, so every `prefix\`...\`` form compiles as Molang whatever the
//! prefix says (`ExpressionRegistry.compile` falls back to the default engine for an unknown prefix); the
//! JSR223 JavaScript engine of the API is code nothing registers. Supported here: statements separated by
//! `;`, `return`, `break`, `continue`, `loop(n, { ... })`, `v.x`/`variable.x`/`t.x`/`temp.x` variables
//! (kept in the scope, temporaries cleared every evaluation), arithmetic, comparison, `&&` `||` `!`, the
//! ternary and `??`, strings, the `math.*` functions (degrees, as Molang has them) and the bindings
//! Citizens adds: `mem.*`, `list.*`, `inv.*`, `item.from_component`, `papi`.
//! Unknown names read as 0, a division by zero is 0 (Molang).
//!
//! Nothing here calls the game: what an expression asks of the NPC's inventory is collected in
//! [`Scope::effects`] for the caller to apply, `papi` goes through a function the glue sets.

use crate::horizon_citizens_core::yaml::Node;
use std::collections::BTreeMap;

/// A value an expression computes or the memory keeps.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Num(f64),
    Bool(bool),
    Str(String),
    /// `papi('...')`: a string that reads as a number when it looks like one (`NumberParseableValue`).
    Parse(String),
    /// An item (`item.from_component`): namespaced id and count.
    Item(String, i64),
    /// A memory list (primitives only).
    List(Vec<Value>),
}

impl Value {
    pub fn num(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Str(s) | Value::Parse(s) => s.trim().parse().unwrap_or(0.0),
            Value::Item(..) | Value::List(_) => 0.0,
        }
    }

    pub fn numeric(&self) -> bool {
        match self {
            Value::Num(_) | Value::Bool(_) => true,
            Value::Str(s) | Value::Parse(s) => s.trim().parse::<f64>().is_ok(),
            Value::Item(..) | Value::List(_) => false,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Num(n) => *n != 0.0,
            Value::Bool(b) => *b,
            Value::Str(_) => true,
            Value::Parse(s) => match s.trim().parse::<f64>() {
                Ok(n) => n != 0.0,
                Err(_) => s.trim().eq_ignore_ascii_case("true") || !s.is_empty(),
            },
            Value::Item(..) => true,
            Value::List(l) => !l.is_empty(),
        }
    }

    /// The text used to compare values (whole numbers without `.0`).
    pub fn text(&self) -> String {
        match self {
            Value::Num(n) => {
                if *n == n.trunc() && n.abs() < 1e15 { format!("{}", *n as i64) } else { n.to_string() }
            }
            Value::Bool(b) => b.to_string(),
            Value::Str(s) | Value::Parse(s) => s.clone(),
            Value::Item(id, n) => format!("{id} x {n}"),
            Value::List(l) => format!("[{}]", l.iter().map(Value::text).collect::<Vec<_>>().join(", ")),
        }
    }

    /// `evaluateAsString`: `Object.toString()`, a number the way Java prints a double.
    pub fn java_text(&self) -> String {
        match self {
            Value::Num(n) => crate::horizon_citizens_core::shop::java_double(*n),
            other => other.text(),
        }
    }
}

/// `Memory`: the behavior tree's persistent blackboard. Primitives are saved, lists are not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Memory {
    map: BTreeMap<String, Value>,
}

impl Memory {
    pub fn new() -> Memory {
        Memory::default()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    pub fn get_number(&self, key: &str, default: f64) -> f64 {
        match self.map.get(key) {
            Some(v @ (Value::Num(_) | Value::Bool(_) | Value::Parse(_))) => v.num(),
            Some(Value::Str(s)) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    pub fn get_string(&self, key: &str, default: &str) -> String {
        self.map.get(key).map_or_else(|| default.to_owned(), Value::java_text)
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.map.get(key) {
            Some(Value::Bool(b)) => *b,
            Some(v) => v.truthy(),
            None => default,
        }
    }

    pub fn has(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn keys(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    /// `Memory.set`: nothing removes the key; an item is not a valid memory type and is left out.
    pub fn set(&mut self, key: &str, value: Value) {
        if matches!(value, Value::Item(..)) {
            return;
        }
        self.map.insert(key.to_owned(), value);
    }

    pub fn size(&self) -> usize {
        self.map.len()
    }

    fn list_mut(&mut self, key: &str) -> &mut Vec<Value> {
        let e = self.map.entry(key.to_owned()).or_insert_with(|| Value::List(Vec::new()));
        if !matches!(e, Value::List(_)) {
            *e = Value::List(Vec::new());
        }
        match e {
            Value::List(l) => l,
            _ => unreachable!("just made a list"),
        }
    }

    pub fn list_add(&mut self, key: &str, v: Value) {
        self.list_mut(key).push(v);
    }

    pub fn list_clear(&mut self, key: &str) {
        self.list_mut(key).clear();
    }

    pub fn list_contains(&self, key: &str, v: &Value) -> bool {
        matches!(self.map.get(key), Some(Value::List(l)) if l.iter().any(|x| same(x, v)))
    }

    pub fn list_get(&self, key: &str, i: i64) -> Option<Value> {
        match self.map.get(key) {
            Some(Value::List(l)) => usize::try_from(i).ok().and_then(|i| l.get(i)).cloned(),
            _ => None,
        }
    }

    pub fn list_remove(&mut self, key: &str, v: &Value) -> bool {
        let l = self.list_mut(key);
        match l.iter().position(|x| same(x, v)) {
            Some(p) => {
                l.remove(p);
                true
            }
            None => false,
        }
    }

    pub fn list_remove_at(&mut self, key: &str, i: i64) -> Option<Value> {
        let l = self.list_mut(key);
        match usize::try_from(i) {
            Ok(i) if i < l.len() => Some(l.remove(i)),
            _ => None,
        }
    }

    pub fn list_size(&self, key: &str) -> usize {
        match self.map.get(key) {
            Some(Value::List(l)) => l.len(),
            _ => 0,
        }
    }

    /// `loadFrom`: the saved primitives of a `memory` map.
    pub fn load_from(&mut self, node: &Node) {
        for e in node.entries() {
            let v = &e.value;
            let value = if let Some(b) = v.as_bool() {
                Value::Bool(b)
            } else if let Some(n) = v.as_f64() {
                Value::Num(n)
            } else if let Some(s) = v.as_str() {
                Value::Str(s.to_owned())
            } else {
                continue;
            };
            self.map.insert(e.key.clone(), value);
        }
    }

    /// `saveTo`: every primitive; lists are not persisted.
    pub fn save_to(&self) -> Node {
        let mut out = Node::map();
        for (k, v) in &self.map {
            let n = match v {
                Value::Num(n) => {
                    if *n == n.trunc() && n.abs() < 9.0e15 { Node::int(*n as i64) } else { Node::float(*n) }
                }
                Value::Bool(b) => Node::bool(*b),
                Value::Str(s) | Value::Parse(s) => Node::str(s),
                Value::Item(..) | Value::List(_) => continue,
            };
            out.set(k, n);
        }
        out
    }
}

fn same(a: &Value, b: &Value) -> bool {
    if a.numeric() && b.numeric() { a.num() == b.num() } else { a.text() == b.text() }
}

static PAPI: std::sync::OnceLock<fn(Option<u128>, &str) -> String> = std::sync::OnceLock::new();

/// The glue's PlaceholderAPI call, used by `papi(...)` in every scope made afterwards.
pub fn set_papi(f: fn(Option<u128>, &str) -> String) {
    let _ = PAPI.set(f);
}

/// What an expression did to the NPC outside its scope; the caller applies these in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Fx {
    InvAdd { id: String, count: i64 },
    InvRemove { id: String, count: i64 },
    InvClear,
    SetHand { id: String, count: i64 },
    SetOffhand { id: String, count: i64 },
    Equip { slot: String, id: String, count: i64 },
}

/// `ExpressionScope` plus what the Molang bindings reach: the variables, the memory, the NPC's inventory.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    /// Dotted names in lower case (`npc.x`, `nearby.player`).
    pub vars: BTreeMap<String, Value>,
    /// `v.`/`variable.` values, kept between evaluations.
    pub locals: BTreeMap<String, Value>,
    pub memory: Memory,
    /// The NPC's inventory as (namespaced id, count), empty slots left out; edited by `inv.*`.
    pub inv: Vec<(String, i64)>,
    /// The NPC's main hand item.
    pub hand: Option<String>,
    pub effects: Vec<Fx>,
    /// The player `papi` fills placeholders for.
    pub player: Option<u128>,
    /// `Placeholders.replace`: the glue's PlaceholderAPI call (none: the text is kept).
    pub papi: Option<fn(Option<u128>, &str) -> String>,
    /// `mem.*` and `list.*` read 0 and change nothing (the scope has no memory, as the `eval` command's).
    pub no_memory: bool,
    /// `inv.*` reads 0 and changes nothing (no NPC in the scope).
    pub no_inv: bool,
    /// Plain replacements `papi` makes before the placeholder call (a shop's `<player>`, `<world>` ...).
    pub subst: Vec<(String, String)>,
    seed: u64,
}

impl Scope {
    pub fn new() -> Scope {
        Scope { seed: 0x9E37_79B9_7F4A_7C15, papi: PAPI.get().copied(), ..Scope::default() }
    }

    pub fn set(&mut self, name: &str, v: Value) {
        self.vars.insert(name.to_ascii_lowercase(), v);
    }

    fn next(&mut self) -> f64 {
        if self.seed == 0 {
            self.seed = 0x9E37_79B9_7F4A_7C15;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ---- the registry's text forms ----------------------------------------------------------------

/// `ExpressionRegistry.isPossiblyExpression`: `^([a-zA-Z_]*)\`(.+)\`$`.
pub fn is_expression(s: &str) -> bool {
    split_expression(s).is_some()
}

/// The prefix (lower case) and the body of a backtick expression.
pub fn split_expression(s: &str) -> Option<(String, String)> {
    let tick = s.find('`')?;
    let prefix = &s[..tick];
    if !prefix.chars().all(|c| c.is_ascii_alphabetic() || c == '_') || !s.ends_with('`') || s.len() < tick + 3 {
        return None;
    }
    Some((prefix.to_ascii_lowercase(), s[tick + 1..s.len() - 1].to_owned()))
}

/// `applyDefaultExpressionMarkup`: wraps a plain text in backticks.
pub fn default_markup(s: &str) -> String {
    if is_expression(s) { s.to_owned() } else { format!("`{s}`") }
}

/// `ExpressionRegistry.compile`: the body of a backtick expression, whatever its prefix (only Molang is registered).
pub fn compile_markup(s: &str) -> Result<Compiled, String> {
    let (_, body) = split_expression(s).ok_or_else(|| format!("Invalid expression syntax: {s}"))?;
    compile(&body)
}

/// `ExpressionValue`: a literal, or a compiled expression.
#[derive(Clone, Debug)]
pub enum ExprValue {
    Literal(String),
    Expr(Compiled),
}

impl ExprValue {
    /// `ExpressionRegistry.parseValue`: a backtick text that compiles is an expression, anything else a literal.
    pub fn parse(s: &str) -> ExprValue {
        if is_expression(s) {
            match compile_markup(s) {
                Ok(c) => return ExprValue::Expr(c),
                Err(e) => eprintln!("[citizens] expression {s}: {e}"),
            }
        }
        ExprValue::Literal(s.to_owned())
    }

    pub fn evaluate(&self, sc: &mut Scope) -> Value {
        match self {
            ExprValue::Literal(s) => Value::Str(s.clone()),
            ExprValue::Expr(c) => c.eval(sc),
        }
    }

    pub fn as_bool(&self, sc: &mut Scope) -> bool {
        match self {
            ExprValue::Literal(s) => s.eq_ignore_ascii_case("true") || !s.is_empty(),
            ExprValue::Expr(c) => c.eval(sc).truthy(),
        }
    }

    pub fn as_number(&self, sc: &mut Scope) -> f64 {
        match self {
            ExprValue::Literal(s) => s.trim().parse().unwrap_or(0.0),
            ExprValue::Expr(c) => c.eval(sc).num(),
        }
    }

    pub fn as_string(&self, sc: &mut Scope) -> String {
        match self {
            ExprValue::Literal(s) => s.clone(),
            ExprValue::Expr(c) => c.eval(sc).java_text(),
        }
    }
}

// ---- durations ----------------------------------------------------------------------------------

/// `SpigotUtil.parseTicks`: a bare integer is ticks, else `20t`, `5s`, `1m30s`, `2h`, `1d`, `500ms`;
/// -1 when it does not read.
pub fn parse_ticks(raw: &str) -> i64 {
    let raw = raw.trim();
    if raw.is_empty() {
        return -1;
    }
    if let Ok(n) = raw.parse::<i64>() {
        return n;
    }
    let lower = raw.to_ascii_lowercase().replace("min", "m").replace("hr", "h");
    let mut ms = 0.0f64;
    let mut num = String::new();
    let mut chars = lower.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' || c == '-' {
            num.push(c);
            continue;
        }
        if num.is_empty() {
            return -1;
        }
        let n: f64 = match num.parse() {
            Ok(n) => n,
            Err(_) => return -1,
        };
        num.clear();
        any = true;
        match c {
            't' => ms += n * 50.0,
            's' => ms += n * 1000.0,
            'm' => {
                if chars.peek() == Some(&'s') {
                    chars.next();
                    ms += n;
                } else {
                    ms += n * 60_000.0;
                }
            }
            'h' => ms += n * 3_600_000.0,
            'd' => ms += n * 86_400_000.0,
            _ => return -1,
        }
    }
    if !num.is_empty() || !any {
        return -1;
    }
    (ms / 50.0) as i64
}

/// `SpigotUtil.parseDuration(raw, MILLISECONDS)` in milliseconds: a bare number is milliseconds.
pub fn parse_millis(raw: &str) -> i64 {
    let raw = raw.trim();
    if let Ok(n) = raw.parse::<i64>() {
        return n;
    }
    let t = parse_ticks(raw);
    if t < 0 { 0 } else { t * 50 }
}

// ---- the parser -------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Ex {
    Num(f64),
    Str(String),
    Var(String),
    Call(String, Vec<Ex>),
    Neg(Box<Ex>),
    Not(Box<Ex>),
    Bin(&'static str, Box<Ex>, Box<Ex>),
    Tern(Box<Ex>, Box<Ex>, Option<Box<Ex>>),
    Coalesce(Box<Ex>, Box<Ex>),
    Assign(String, Box<Ex>),
    Block(Vec<Ex>),
    Return(Option<Box<Ex>>),
    Break,
    Continue,
}

/// A compiled Molang expression.
#[derive(Clone, Debug)]
pub struct Compiled {
    prog: Vec<Ex>,
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

/// `MolangEngine.compile`.
pub fn compile(src: &str) -> Result<Compiled, String> {
    let mut p = Parser { s: src.chars().collect(), i: 0 };
    let prog = p.program(false)?;
    p.ws();
    if p.i < p.s.len() {
        return Err(format!("Failed to parse Molang expression: {src}"));
    }
    Ok(Compiled { prog })
}

impl Parser {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        let t: Vec<char> = tok.chars().collect();
        if self.s[self.i..].starts_with(&t) {
            self.i += t.len();
            true
        } else {
            false
        }
    }

    fn peek(&mut self, tok: &str) -> bool {
        let save = self.i;
        let r = self.eat(tok);
        self.i = save;
        r
    }

    fn program(&mut self, in_block: bool) -> Result<Vec<Ex>, String> {
        let mut out = Vec::new();
        loop {
            self.ws();
            if self.i >= self.s.len() || (in_block && self.peek("}")) {
                break;
            }
            if self.eat(";") {
                continue;
            }
            out.push(self.stmt()?);
            self.ws();
            if self.i < self.s.len() && !self.peek(";") && !(in_block && self.peek("}")) {
                return Err("expected ;".to_owned());
            }
        }
        Ok(out)
    }

    fn stmt(&mut self) -> Result<Ex, String> {
        let save = self.i;
        if self.word_at_ci("return") {
            self.ws();
            if self.i >= self.s.len() || self.peek(";") || self.peek("}") {
                return Ok(Ex::Return(None));
            }
            return Ok(Ex::Return(Some(Box::new(self.expr()?))));
        }
        if self.word_at_ci("break") {
            return Ok(Ex::Break);
        }
        if self.word_at_ci("continue") {
            return Ok(Ex::Continue);
        }
        self.i = save;
        self.expr()
    }

    fn word_at_ci(&mut self, w: &str) -> bool {
        self.ws();
        let t: Vec<char> = w.chars().collect();
        if self.i + t.len() > self.s.len() {
            return false;
        }
        let head: String = self.s[self.i..self.i + t.len()].iter().collect();
        if !head.eq_ignore_ascii_case(w) {
            return false;
        }
        let after = self.s.get(self.i + t.len()).copied();
        if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            return false;
        }
        self.i += t.len();
        true
    }

    fn expr(&mut self) -> Result<Ex, String> {
        let l = self.ternary()?;
        if let Ex::Var(name) = &l {
            self.ws();
            if self.peek("=") && !self.peek("==") {
                self.eat("=");
                let r = self.expr()?;
                return Ok(Ex::Assign(name.clone(), Box::new(r)));
            }
        }
        Ok(l)
    }

    fn ternary(&mut self) -> Result<Ex, String> {
        let c = self.coalesce()?;
        if self.peek("?") && !self.peek("??") {
            self.eat("?");
            let a = self.ternary()?;
            let b = if self.eat(":") { Some(Box::new(self.ternary()?)) } else { None };
            return Ok(Ex::Tern(Box::new(c), Box::new(a), b));
        }
        Ok(c)
    }

    fn coalesce(&mut self) -> Result<Ex, String> {
        let l = self.or()?;
        if self.eat("??") {
            let r = self.coalesce()?;
            return Ok(Ex::Coalesce(Box::new(l), Box::new(r)));
        }
        Ok(l)
    }

    fn or(&mut self) -> Result<Ex, String> {
        let mut l = self.and()?;
        while self.eat("||") {
            let r = self.and()?;
            l = Ex::Bin("||", Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<Ex, String> {
        let mut l = self.eq()?;
        while self.eat("&&") {
            let r = self.eq()?;
            l = Ex::Bin("&&", Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn eq(&mut self) -> Result<Ex, String> {
        let mut l = self.rel()?;
        loop {
            let op = if self.eat("==") {
                "=="
            } else if self.eat("!=") {
                "!="
            } else {
                return Ok(l);
            };
            let r = self.rel()?;
            l = Ex::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn rel(&mut self) -> Result<Ex, String> {
        let mut l = self.add()?;
        loop {
            let op = if self.eat("<=") {
                "<="
            } else if self.eat(">=") {
                ">="
            } else if self.eat("<") {
                "<"
            } else if self.eat(">") {
                ">"
            } else {
                return Ok(l);
            };
            let r = self.add()?;
            l = Ex::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn add(&mut self) -> Result<Ex, String> {
        let mut l = self.mul()?;
        loop {
            let op = if self.eat("+") {
                "+"
            } else if self.eat("-") {
                "-"
            } else {
                return Ok(l);
            };
            let r = self.mul()?;
            l = Ex::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn mul(&mut self) -> Result<Ex, String> {
        let mut l = self.unary()?;
        loop {
            let op = if self.eat("*") {
                "*"
            } else if self.eat("/") {
                "/"
            } else if self.eat("%") {
                "%"
            } else {
                return Ok(l);
            };
            let r = self.unary()?;
            l = Ex::Bin(op, Box::new(l), Box::new(r));
        }
    }

    fn unary(&mut self) -> Result<Ex, String> {
        if self.peek("!") && !self.peek("!=") {
            self.eat("!");
            return Ok(Ex::Not(Box::new(self.unary()?)));
        }
        if self.eat("-") {
            return Ok(Ex::Neg(Box::new(self.unary()?)));
        }
        if self.eat("+") {
            return self.unary();
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Ex, String> {
        self.ws();
        let Some(&c) = self.s.get(self.i) else { return Err("unexpected end".to_owned()) };
        if c == '(' {
            self.i += 1;
            let v = self.expr()?;
            return if self.eat(")") { Ok(v) } else { Err("expected )".to_owned()) };
        }
        if c == '{' {
            self.i += 1;
            let body = self.program(true)?;
            return if self.eat("}") { Ok(Ex::Block(body)) } else { Err("expected }".to_owned()) };
        }
        if c == '\'' {
            self.i += 1;
            let start = self.i;
            while self.i < self.s.len() && self.s[self.i] != '\'' {
                self.i += 1;
            }
            if self.i >= self.s.len() {
                return Err("unterminated string".to_owned());
            }
            let t: String = self.s[start..self.i].iter().collect();
            self.i += 1;
            return Ok(Ex::Str(t));
        }
        if c.is_ascii_digit() || c == '.' {
            let start = self.i;
            while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == '.') {
                self.i += 1;
            }
            let t: String = self.s[start..self.i].iter().collect();
            return t.parse().map(Ex::Num).map_err(|_| format!("bad number {t}"));
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = self.i;
            while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == '_' || self.s[self.i] == '.') {
                self.i += 1;
            }
            let name: String = self.s[start..self.i].iter().collect::<String>().to_ascii_lowercase();
            match name.as_str() {
                "break" => return Ok(Ex::Break),
                "continue" => return Ok(Ex::Continue),
                "return" => {
                    self.ws();
                    if self.i >= self.s.len() || self.peek(";") || self.peek("}") || self.peek(")") {
                        return Ok(Ex::Return(None));
                    }
                    return Ok(Ex::Return(Some(Box::new(self.expr()?))));
                }
                _ => {}
            }
            if self.peek("(") {
                self.eat("(");
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(",") {
                            continue;
                        }
                        if self.eat(")") {
                            break;
                        }
                        return Err("expected , or )".to_owned());
                    }
                }
                return Ok(Ex::Call(name, args));
            }
            return Ok(Ex::Var(name));
        }
        Err(format!("unexpected {c}"))
    }
}

// ---- evaluation -------------------------------------------------------------------------------

struct Run<'a> {
    sc: &'a mut Scope,
    temps: BTreeMap<String, Value>,
    ret: Option<Value>,
    brk: bool,
    cont: bool,
    steps: u32,
}

/// The longest a loop may spin (Mocha caps loops as well).
const MAX_STEPS: u32 = 100_000;

impl Compiled {
    /// `evaluate`: the value of the last statement, or of the `return`.
    pub fn eval(&self, sc: &mut Scope) -> Value {
        let mut r = Run { sc, temps: BTreeMap::new(), ret: None, brk: false, cont: false, steps: 0 };
        let v = r.block(&self.prog);
        r.ret.take().unwrap_or(v)
    }

    pub fn eval_bool(&self, sc: &mut Scope) -> bool {
        self.eval(sc).truthy()
    }

    pub fn eval_number(&self, sc: &mut Scope) -> f64 {
        self.eval(sc).num()
    }

    pub fn eval_string(&self, sc: &mut Scope) -> String {
        self.eval(sc).java_text()
    }
}

fn flag(b: bool) -> Value {
    Value::Num(if b { 1.0 } else { 0.0 })
}

impl Run<'_> {
    fn done(&self) -> bool {
        self.ret.is_some() || self.brk || self.cont
    }

    fn block(&mut self, prog: &[Ex]) -> Value {
        let mut last = Value::Num(0.0);
        for e in prog {
            last = self.ev(e);
            if self.done() {
                break;
            }
        }
        last
    }

    fn lookup(&mut self, name: &str) -> Value {
        for p in ["t.", "temp.", "temporary."] {
            if let Some(rest) = name.strip_prefix(p) {
                return self.temps.get(rest).cloned().unwrap_or(Value::Num(0.0));
            }
        }
        for p in ["v.", "variable."] {
            if let Some(rest) = name.strip_prefix(p) {
                return self.sc.locals.get(rest).cloned().unwrap_or(Value::Num(0.0));
            }
        }
        match name {
            "true" => return Value::Num(1.0),
            "false" => return Value::Num(0.0),
            "math.pi" => return Value::Num(std::f64::consts::PI),
            _ => {}
        }
        self.sc.vars.get(name).cloned().unwrap_or(Value::Num(0.0))
    }

    fn assign(&mut self, name: &str, v: Value) {
        for p in ["t.", "temp.", "temporary."] {
            if let Some(rest) = name.strip_prefix(p) {
                self.temps.insert(rest.to_owned(), v);
                return;
            }
        }
        for p in ["v.", "variable."] {
            if let Some(rest) = name.strip_prefix(p) {
                self.sc.locals.insert(rest.to_owned(), v);
                return;
            }
        }
        // Anything else is a binding Mocha keeps in its scope too: kept as a variable of the scope.
        self.sc.vars.insert(name.to_owned(), v);
    }

    fn ev(&mut self, e: &Ex) -> Value {
        if self.done() {
            return Value::Num(0.0);
        }
        match e {
            Ex::Num(n) => Value::Num(*n),
            Ex::Str(s) => Value::Str(s.clone()),
            Ex::Var(n) => self.lookup(n),
            Ex::Neg(x) => Value::Num(-self.ev(x).num()),
            Ex::Not(x) => flag(!self.ev(x).truthy()),
            Ex::Assign(n, x) => {
                let v = self.ev(x);
                self.assign(n, v.clone());
                v
            }
            Ex::Block(b) => self.block(b),
            Ex::Return(x) => {
                let v = x.as_ref().map_or(Value::Num(0.0), |x| self.ev(x));
                self.ret = Some(v.clone());
                v
            }
            Ex::Break => {
                self.brk = true;
                Value::Num(0.0)
            }
            Ex::Continue => {
                self.cont = true;
                Value::Num(0.0)
            }
            Ex::Tern(c, a, b) => {
                if self.ev(c).truthy() {
                    self.ev(a)
                } else {
                    b.as_ref().map_or(Value::Num(0.0), |b| self.ev(b))
                }
            }
            Ex::Coalesce(a, b) => {
                // `a ?? b`: b when a is an unset variable (reads as 0 here, which Molang calls null).
                let named_unset = match &**a {
                    Ex::Var(n) => !self.is_set(n),
                    _ => false,
                };
                if named_unset { self.ev(b) } else { self.ev(a) }
            }
            Ex::Bin(op, a, b) => self.bin(op, a, b),
            Ex::Call(name, args) => self.call(name, args),
        }
    }

    fn is_set(&mut self, name: &str) -> bool {
        for p in ["t.", "temp.", "temporary."] {
            if let Some(rest) = name.strip_prefix(p) {
                return self.temps.contains_key(rest);
            }
        }
        for p in ["v.", "variable."] {
            if let Some(rest) = name.strip_prefix(p) {
                return self.sc.locals.contains_key(rest);
            }
        }
        self.sc.vars.contains_key(name)
    }

    fn bin(&mut self, op: &str, a: &Ex, b: &Ex) -> Value {
        match op {
            "||" => {
                let l = self.ev(a);
                if l.truthy() {
                    return flag(true);
                }
                flag(self.ev(b).truthy())
            }
            "&&" => {
                let l = self.ev(a);
                if !l.truthy() {
                    return flag(false);
                }
                flag(self.ev(b).truthy())
            }
            _ => {
                let l = self.ev(a);
                let r = self.ev(b);
                match op {
                    "==" | "!=" => {
                        let same = if l.numeric() && r.numeric() { l.num() == r.num() } else { l.text() == r.text() };
                        flag(same == (op == "=="))
                    }
                    "<" => flag(l.num() < r.num()),
                    ">" => flag(l.num() > r.num()),
                    "<=" => flag(l.num() <= r.num()),
                    ">=" => flag(l.num() >= r.num()),
                    "+" => Value::Num(l.num() + r.num()),
                    "-" => Value::Num(l.num() - r.num()),
                    "*" => Value::Num(l.num() * r.num()),
                    "/" => Value::Num(if r.num() == 0.0 { 0.0 } else { l.num() / r.num() }),
                    _ => Value::Num(if r.num() == 0.0 { 0.0 } else { l.num() % r.num() }),
                }
            }
        }
    }

    fn args(&mut self, args: &[Ex]) -> Vec<Value> {
        args.iter().map(|a| self.ev(a)).collect()
    }

    fn call(&mut self, name: &str, args: &[Ex]) -> Value {
        if name == "loop" {
            return self.run_loop(args);
        }
        let a = self.args(args);
        let n = |i: usize| a.get(i).map_or(0.0, Value::num);
        let t = |i: usize| a.get(i).map(Value::text).unwrap_or_default();
        if let Some(m) = name.strip_prefix("math.") {
            return self.math(m, &a);
        }
        if let Some(m) = name.strip_prefix("mem.") {
            return self.mem(m, &a);
        }
        if let Some(m) = name.strip_prefix("list.") {
            return self.list(m, &a);
        }
        if let Some(m) = name.strip_prefix("inv.") {
            return self.inv(m, &a);
        }
        match name {
            "papi" => {
                let raw = t(0);
                let player = self.sc.player;
                let mut text = raw;
                for (from, to) in &self.sc.subst {
                    text = text.replace(from.as_str(), to);
                }
                if let Some(f) = self.sc.papi {
                    text = f(player, &text);
                }
                Value::Parse(text)
            }
            "item.from_component" => {
                if a.is_empty() {
                    return Value::Num(0.0);
                }
                let amount = if a.len() > 1 { n(1) as i64 } else { 1 };
                Value::Item(item_id(&t(0)), amount.max(1))
            }
            _ => Value::Num(0.0),
        }
    }

    fn run_loop(&mut self, args: &[Ex]) -> Value {
        if args.len() < 2 {
            return Value::Num(0.0);
        }
        let count = self.ev(&args[0]).num().max(0.0) as u64;
        for _ in 0..count {
            self.steps += 1;
            if self.steps > MAX_STEPS {
                break;
            }
            self.ev(&args[1]);
            self.cont = false;
            if self.brk {
                self.brk = false;
                break;
            }
            if self.ret.is_some() {
                break;
            }
        }
        Value::Num(0.0)
    }

    fn math(&mut self, f: &str, a: &[Value]) -> Value {
        let n = |i: usize| a.get(i).map_or(0.0, Value::num);
        let rad = |d: f64| d.to_radians();
        let v = match f {
            "abs" => n(0).abs(),
            "acos" => n(0).acos().to_degrees(),
            "asin" => n(0).asin().to_degrees(),
            "atan" => n(0).atan().to_degrees(),
            "atan2" => n(0).atan2(n(1)).to_degrees(),
            "ceil" => n(0).ceil(),
            "clamp" => n(0).max(n(1)).min(n(2)),
            "cos" => rad(n(0)).cos(),
            "sin" => rad(n(0)).sin(),
            "exp" => n(0).exp(),
            "floor" => n(0).floor(),
            "hermite_blend" => {
                let x = n(0);
                3.0 * x * x - 2.0 * x * x * x
            }
            "lerp" => n(0) + (n(1) - n(0)) * n(2),
            "lerprotate" => {
                let (s, e, t) = (n(0), n(1), n(2));
                let mut d = (e - s) % 360.0;
                if d > 180.0 {
                    d -= 360.0;
                } else if d < -180.0 {
                    d += 360.0;
                }
                s + d * t
            }
            "ln" => n(0).ln(),
            "max" => n(0).max(n(1)),
            "min" => n(0).min(n(1)),
            "min_angle" => {
                let mut d = n(0) % 360.0;
                if d >= 180.0 {
                    d -= 360.0;
                } else if d < -180.0 {
                    d += 360.0;
                }
                d
            }
            "mod" => {
                if n(1) == 0.0 { 0.0 } else { n(0) % n(1) }
            }
            "pi" => std::f64::consts::PI,
            "pow" => n(0).powf(n(1)),
            "random" => {
                let r = self.sc.next();
                n(0) + (n(1) - n(0)) * r
            }
            "random_integer" => {
                let r = self.sc.next();
                (n(0) + ((n(1) - n(0) + 1.0) * r).floor()).min(n(1).max(n(0)))
            }
            "die_roll" | "die_roll_integer" => {
                let mut total = 0.0;
                for _ in 0..(n(0).max(0.0) as i64).min(10_000) {
                    let r = self.sc.next();
                    let x = n(1) + (n(2) - n(1)) * r;
                    total += if f == "die_roll_integer" { x.floor() } else { x };
                }
                total
            }
            "round" => n(0).round(),
            "sqrt" => n(0).sqrt(),
            "trunc" => n(0).trunc(),
            "copy_sign" => n(0).abs().copysign(n(1)),
            _ => 0.0,
        };
        Value::Num(if v.is_finite() { v } else { 0.0 })
    }

    fn mem(&mut self, f: &str, a: &[Value]) -> Value {
        if self.sc.no_memory {
            return Value::Num(0.0);
        }
        let key = a.first().map(Value::text);
        match (f, key) {
            ("set", Some(k)) if a.len() >= 2 => {
                let v = a[1].num();
                self.sc.memory.set(&k, Value::Num(v));
                Value::Num(v)
            }
            ("get", Some(k)) => {
                let d = a.get(1).map_or(0.0, Value::num);
                Value::Num(self.sc.memory.get_number(&k, d))
            }
            ("has", Some(k)) => flag(self.sc.memory.has(&k)),
            ("remove", Some(k)) => {
                self.sc.memory.remove(&k);
                Value::Num(1.0)
            }
            _ => Value::Num(0.0),
        }
    }

    fn list(&mut self, f: &str, a: &[Value]) -> Value {
        if self.sc.no_memory {
            return Value::Num(0.0);
        }
        let Some(k) = a.first().map(Value::text) else { return Value::Num(0.0) };
        let v = a.get(1).map(|v| Value::Num(v.num()));
        let m = &mut self.sc.memory;
        match f {
            "add" => match v {
                Some(v) => {
                    m.list_add(&k, v);
                    Value::Num(1.0)
                }
                None => Value::Num(0.0),
            },
            "remove" => v.map_or(Value::Num(0.0), |v| flag(m.list_remove(&k, &v))),
            "remove_at" => {
                let i = a.get(1).map_or(0.0, Value::num) as i64;
                flag(m.list_remove_at(&k, i).is_some())
            }
            "clear" => {
                m.list_clear(&k);
                Value::Num(1.0)
            }
            "size" => Value::Num(m.list_size(&k) as f64),
            "get" => {
                let i = a.get(1).map_or(0.0, Value::num) as i64;
                match m.list_get(&k, i) {
                    Some(x @ Value::Num(_)) => x,
                    _ => Value::Num(0.0),
                }
            }
            "contains" => v.map_or(Value::Num(0.0), |v| flag(m.list_contains(&k, &v))),
            _ => Value::Num(0.0),
        }
    }

    fn inv(&mut self, f: &str, a: &[Value]) -> Value {
        if self.sc.no_inv {
            return Value::Num(0.0);
        }
        let item = |i: usize, count: i64| -> Option<(String, i64)> {
            match a.get(i)? {
                Value::Item(id, n) => Some((id.clone(), if count > 0 { count } else { *n })),
                v => {
                    let t = v.text();
                    if t.is_empty() { None } else { Some((item_id(&t), count.max(1))) }
                }
            }
        };
        let amount = |i: usize| a.get(i).map_or(1, |v| (v.num() as i64).max(1));
        match f {
            "has" => match item(0, 0) {
                Some((id, _)) => flag(self.sc.inv.iter().any(|(i, _)| *i == id)),
                None => Value::Num(0.0),
            },
            "count" => match item(0, 0) {
                Some((id, _)) => Value::Num(self.sc.inv.iter().filter(|(i, _)| *i == id).map(|(_, c)| *c as f64).sum()),
                None => Value::Num(0.0),
            },
            "add" => match item(0, if a.len() > 1 { amount(1) } else { 0 }) {
                Some((id, count)) => {
                    self.sc.inv.push((id.clone(), count));
                    self.sc.effects.push(Fx::InvAdd { id, count });
                    Value::Num(1.0)
                }
                None => Value::Num(0.0),
            },
            "remove" => match item(0, if a.len() > 1 { amount(1) } else { 1 }) {
                Some((id, count)) => {
                    let mut left = count;
                    for (i, c) in self.sc.inv.iter_mut() {
                        if left > 0 && *i == id {
                            let take = (*c).min(left);
                            *c -= take;
                            left -= take;
                        }
                    }
                    self.sc.inv.retain(|(_, c)| *c > 0);
                    self.sc.effects.push(Fx::InvRemove { id, count });
                    flag(left == 0)
                }
                None => Value::Num(0.0),
            },
            "clear" => {
                self.sc.inv.clear();
                self.sc.effects.push(Fx::InvClear);
                Value::Num(1.0)
            }
            "set_hand" => match item(0, 0) {
                Some((id, count)) => {
                    self.sc.hand = Some(id.clone());
                    self.sc.effects.push(Fx::SetHand { id, count });
                    Value::Num(1.0)
                }
                None => Value::Num(0.0),
            },
            "set_offhand" => match item(0, 0) {
                Some((id, count)) => {
                    self.sc.effects.push(Fx::SetOffhand { id, count });
                    Value::Num(1.0)
                }
                None => Value::Num(0.0),
            },
            "hand_is" => match item(0, 0) {
                Some((id, _)) => flag(self.sc.hand.as_deref() == Some(id.as_str())),
                None => Value::Num(0.0),
            },
            "equip" => {
                if a.len() < 2 {
                    return Value::Num(0.0);
                }
                let slot = a[0].text().to_ascii_lowercase();
                let known = ["hand", "off_hand", "head", "chest", "legs", "feet", "body", "saddle", "helmet", "chestplate", "leggings", "boots"];
                match (known.contains(&slot.as_str()), item(1, 0)) {
                    (true, Some((id, count))) => {
                        self.sc.effects.push(Fx::Equip { slot, id, count });
                        Value::Num(1.0)
                    }
                    _ => Value::Num(0.0),
                }
            }
            _ => Value::Num(0.0),
        }
    }
}

/// `Material.matchMaterial`: a name in any case, with or without the namespace, as a namespaced id.
pub fn item_id(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase().replace(' ', "_");
    if n.contains(':') { n } else { format!("minecraft:{n}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Value {
        let c = compile(src).expect("compiles");
        let mut sc = Scope::new();
        c.eval(&mut sc)
    }

    fn run_in(src: &str, sc: &mut Scope) -> Value {
        compile(src).expect("compiles").eval(sc)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(run("1 + 2 * 3"), Value::Num(7.0));
        assert_eq!(run("(1 + 2) * 3"), Value::Num(9.0));
        assert_eq!(run("10 / 0"), Value::Num(0.0));
        assert_eq!(run("-3 + 5"), Value::Num(2.0));
        assert_eq!(run("7 % 4"), Value::Num(3.0));
    }

    #[test]
    fn comparison_logic_and_ternary() {
        assert!(run("1 < 2 && 3 >= 3").truthy());
        assert!(!run("1 > 2 || 0").truthy());
        assert_eq!(run("1 > 2 ? 5 : 6"), Value::Num(6.0));
        assert!(run("'a' == 'a'").truthy());
        assert!(!run("'a' != 'a'").truthy());
        assert!(run("!0").truthy());
    }

    #[test]
    fn statements_variables_and_return() {
        assert_eq!(run("v.x = 4; v.y = v.x * 2; return v.y + 1"), Value::Num(9.0));
        assert_eq!(run("v.a = 1; 5; v.a + 10"), Value::Num(11.0));
        assert_eq!(run("return 3; 99"), Value::Num(3.0));
    }

    #[test]
    fn locals_survive_but_temporaries_do_not() {
        let mut sc = Scope::new();
        run_in("v.n = 5; t.k = 7", &mut sc);
        assert_eq!(run_in("v.n", &mut sc), Value::Num(5.0));
        assert_eq!(run_in("t.k", &mut sc), Value::Num(0.0));
    }

    #[test]
    fn loops_break_and_continue() {
        assert_eq!(run("v.s = 0; loop(5, { v.s = v.s + 2; }); v.s"), Value::Num(10.0));
        assert_eq!(run("v.s = 0; loop(10, { v.s = v.s + 1; v.s >= 3 ? break; }); v.s"), Value::Num(3.0));
        assert_eq!(run("v.s = 0; loop(4, { v.s = v.s + 1; continue; v.s = 99; }); v.s"), Value::Num(4.0));
    }

    #[test]
    fn math_functions_use_degrees() {
        assert_eq!(run("math.max(2, 9)"), Value::Num(9.0));
        assert_eq!(run("math.clamp(15, 0, 10)"), Value::Num(10.0));
        assert_eq!(run("math.floor(2.7) + math.ceil(2.1)"), Value::Num(5.0));
        assert_eq!(run("math.pow(2, 10)"), Value::Num(1024.0));
        assert!((run("math.sin(90)").num() - 1.0).abs() < 1e-9);
        assert_eq!(run("math.lerp(0, 10, 0.5)"), Value::Num(5.0));
        let r = run("math.random(3, 4)").num();
        assert!((3.0..=4.0).contains(&r));
    }

    #[test]
    fn scope_variables_by_dotted_name() {
        let mut sc = Scope::new();
        sc.set("npc.x", Value::Num(12.0));
        sc.set("nearby.player", Value::Bool(true));
        assert_eq!(run_in("npc.x + 1", &mut sc), Value::Num(13.0));
        assert!(run_in("nearby.player && npc.x > 10", &mut sc).truthy());
        assert_eq!(run_in("npc.unknown", &mut sc), Value::Num(0.0));
    }

    #[test]
    fn coalesce_takes_the_right_side_of_an_unset_name() {
        assert_eq!(run("v.nope ?? 4"), Value::Num(4.0));
        assert_eq!(run("v.a = 2; v.a ?? 4"), Value::Num(2.0));
    }

    #[test]
    fn memory_functions() {
        let mut sc = Scope::new();
        run_in("mem.set('hp', 12)", &mut sc);
        assert_eq!(run_in("mem.get('hp')", &mut sc), Value::Num(12.0));
        assert_eq!(run_in("mem.get('nope', 3)", &mut sc), Value::Num(3.0));
        assert!(run_in("mem.has('hp')", &mut sc).truthy());
        run_in("mem.remove('hp')", &mut sc);
        assert!(!run_in("mem.has('hp')", &mut sc).truthy());
        run_in("list.add('l', 4); list.add('l', 9)", &mut sc);
        assert_eq!(run_in("list.size('l')", &mut sc), Value::Num(2.0));
        assert_eq!(run_in("list.get('l', 1)", &mut sc), Value::Num(9.0));
        assert!(run_in("list.contains('l', 4)", &mut sc).truthy());
        run_in("list.remove_at('l', 0)", &mut sc);
        assert_eq!(run_in("list.size('l')", &mut sc), Value::Num(1.0));
        run_in("list.clear('l')", &mut sc);
        assert_eq!(run_in("list.size('l')", &mut sc), Value::Num(0.0));
    }

    #[test]
    fn inventory_functions_record_their_effects() {
        let mut sc = Scope::new();
        sc.inv.push(("minecraft:apple".into(), 3));
        assert!(run_in("inv.has('apple')", &mut sc).truthy());
        assert_eq!(run_in("inv.count('minecraft:apple')", &mut sc), Value::Num(3.0));
        assert!(run_in("inv.remove('apple', 2)", &mut sc).truthy());
        assert_eq!(run_in("inv.count('apple')", &mut sc), Value::Num(1.0));
        assert!(!run_in("inv.remove('apple', 5)", &mut sc).truthy());
        run_in("inv.add(item.from_component('diamond', 8))", &mut sc);
        assert_eq!(run_in("inv.count('diamond')", &mut sc), Value::Num(8.0));
        run_in("inv.set_hand('stick'); inv.equip('head', 'iron_helmet')", &mut sc);
        assert!(run_in("inv.hand_is('stick')", &mut sc).truthy());
        assert!(sc.effects.contains(&Fx::Equip { slot: "head".into(), id: "minecraft:iron_helmet".into(), count: 1 }));
        run_in("inv.clear()", &mut sc);
        assert_eq!(run_in("inv.count('diamond')", &mut sc), Value::Num(0.0));
    }

    #[test]
    fn papi_reads_as_a_number_when_it_looks_like_one() {
        let mut sc = Scope::new();
        sc.papi = Some(|_, t| if t == "%x%" { "12".to_owned() } else { t.to_owned() });
        assert!(run_in("papi('%x%') > 10", &mut sc).truthy());
        assert!(run_in("papi('%x%') == 12", &mut sc).truthy());
    }

    #[test]
    fn a_prefix_does_not_pick_another_engine() {
        assert_eq!(compile_markup("js`1 + 2`").unwrap().eval(&mut Scope::new()), Value::Num(3.0));
        assert_eq!(compile_markup("`4 * 2`").unwrap().eval(&mut Scope::new()), Value::Num(8.0));
        assert!(compile_markup("not an expression").is_err());
    }

    #[test]
    fn syntax_errors_do_not_compile() {
        assert!(compile("1 +").is_err());
        assert!(compile("(1 + 2").is_err());
        assert!(compile("1 2").is_err());
    }

    #[test]
    fn durations_parse_as_citizens_does() {
        assert_eq!(parse_ticks("20"), 20);
        assert_eq!(parse_ticks("5s"), 100);
        assert_eq!(parse_ticks("1m"), 1200);
        assert_eq!(parse_ticks("1m30s"), 1800);
        assert_eq!(parse_ticks("40t"), 40);
        assert_eq!(parse_ticks("2h"), 144_000);
        assert_eq!(parse_ticks("abc"), -1);
        assert_eq!(parse_millis("5s"), 5000);
    }

    #[test]
    fn memory_saves_primitives_only() {
        let mut m = Memory::new();
        m.set("a", Value::Num(3.0));
        m.set("b", Value::Str("hi".into()));
        m.set("c", Value::Bool(true));
        m.list_add("l", Value::Num(1.0));
        let n = m.save_to();
        assert_eq!(n.get("a").and_then(Node::as_i64), Some(3));
        assert!(n.get("l").is_none());
        let mut back = Memory::new();
        back.load_from(&n);
        assert_eq!(back.get_number("a", 0.0), 3.0);
        assert_eq!(back.get_string("b", ""), "hi");
        assert!(back.get_bool("c", false));
    }
}
