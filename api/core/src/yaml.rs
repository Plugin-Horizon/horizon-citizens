//! A small YAML reader and writer for the subset Bukkit's `YamlConfiguration` writes: block maps and
//! block sequences (a sequence may sit at its key's own indent), plain, single- and double-quoted
//! scalars, empty flow collections (`{}`, `[]`, also spread over lines) and simple flow sequences of
//! scalars, `#` comments.
//!
//! Every scalar and key keeps the text it was read with, so a file read and written again without
//! changes comes out the same (apart from comments inside the tree and empty flow collections,
//! which are written as `{}` / `[]`). New values are written in the style SnakeYAML uses.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Scalar(Scalar),
    Map(Map),
    Seq(Vec<Node>),
}

/// A scalar as written in the file (`raw`, quotes included).
#[derive(Clone, Debug, PartialEq)]
pub struct Scalar {
    pub raw: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map {
    pub entries: Vec<(Key, Node)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    /// The decoded key.
    pub name: String,
    /// The key as written (`'3'` for the decoded `3`).
    pub raw: String,
}

impl Key {
    pub fn new(name: &str) -> Self {
        Key {
            name: name.to_owned(),
            raw: quote_if_needed(name),
        }
    }
}

impl Scalar {
    pub fn str(s: &str) -> Self {
        Scalar {
            raw: quote_if_needed(s),
        }
    }
    pub fn bool(b: bool) -> Self {
        Scalar {
            raw: if b { "true" } else { "false" }.into(),
        }
    }
    pub fn int(i: i64) -> Self {
        Scalar { raw: i.to_string() }
    }
    pub fn float(f: f64) -> Self {
        Scalar { raw: java_double(f) }
    }
    pub fn null() -> Self {
        Scalar { raw: String::new() }
    }
    /// The scalar's text without quotes and escapes.
    pub fn text(&self) -> String {
        unquote(&self.raw)
    }
    pub fn is_null(&self) -> bool {
        let r = self.raw.as_str();
        r.is_empty() || r == "~" || r.eq_ignore_ascii_case("null")
    }
    /// YAML 1.1 booleans (SnakeYAML's): true/false, yes/no, on/off, any case; a quoted `'False'`
    /// counts too, as Citizens reads such strings with `Boolean.parseBoolean`.
    pub fn as_bool(&self) -> Option<bool> {
        match self.text().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "y" => Some(true),
            "false" | "no" | "off" | "n" => Some(false),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        let t = self.text();
        let t = t.trim();
        t.parse::<f64>().ok().filter(|f| f.is_finite())
    }
    pub fn as_i64(&self) -> Option<i64> {
        let t = self.text();
        let t = t.trim();
        t.parse::<i64>()
            .ok()
            .or_else(|| self.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
    }
}

impl Node {
    pub fn str(s: &str) -> Node {
        Node::Scalar(Scalar::str(s))
    }
    pub fn bool(b: bool) -> Node {
        Node::Scalar(Scalar::bool(b))
    }
    pub fn int(i: i64) -> Node {
        Node::Scalar(Scalar::int(i))
    }
    pub fn float(f: f64) -> Node {
        Node::Scalar(Scalar::float(f))
    }
    pub fn map() -> Node {
        Node::Map(Map::default())
    }
    pub fn as_scalar(&self) -> Option<&Scalar> {
        match self {
            Node::Scalar(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Node::Map(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_map_mut(&mut self) -> Option<&mut Map> {
        match self {
            Node::Map(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_seq(&self) -> Option<&Vec<Node>> {
        match self {
            Node::Seq(s) => Some(s),
            _ => None,
        }
    }
    pub fn text(&self) -> Option<String> {
        self.as_scalar().filter(|s| !s.is_null()).map(Scalar::text)
    }
    pub fn as_bool(&self) -> Option<bool> {
        self.as_scalar().and_then(Scalar::as_bool)
    }
    pub fn as_f64(&self) -> Option<f64> {
        self.as_scalar().and_then(Scalar::as_f64)
    }
    pub fn as_i64(&self) -> Option<i64> {
        self.as_scalar().and_then(Scalar::as_i64)
    }
}

impl Map {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries.iter().find(|(k, _)| k.name == key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k.name == key)
            .map(|(_, v)| v)
    }
    /// Sets `key`, keeping its place (and the key's spelling) when it exists, else appending it.
    pub fn set(&mut self, key: &str, value: Node) {
        match self.get_mut(key) {
            Some(v) => *v = value,
            None => self.entries.push((Key::new(key), value)),
        }
    }
    pub fn remove(&mut self, key: &str) -> Option<Node> {
        let i = self.entries.iter().position(|(k, _)| k.name == key)?;
        Some(self.entries.remove(i).1)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.name.as_str())
    }
    pub fn get_path(&self, path: &[&str]) -> Option<&Node> {
        let (first, rest) = path.split_first()?;
        let node = self.get(first)?;
        if rest.is_empty() {
            Some(node)
        } else {
            node.as_map()?.get_path(rest)
        }
    }
    /// The map at `path`, made (and anything in the way replaced) when missing.
    pub fn map_at(&mut self, path: &[&str]) -> &mut Map {
        let Some((first, rest)) = path.split_first() else {
            return self;
        };
        if !matches!(self.get(first), Some(Node::Map(_))) {
            self.set(first, Node::map());
        }
        let Some(Node::Map(m)) = self.get_mut(first) else {
            unreachable!("just set to a map")
        };
        m.map_at(rest)
    }
    pub fn set_path(&mut self, path: &[&str], value: Node) {
        let Some((last, parents)) = path.split_last() else {
            return;
        };
        self.map_at(parents).set(last, value);
    }
    pub fn remove_path(&mut self, path: &[&str]) -> Option<Node> {
        let (last, parents) = path.split_last()?;
        let mut m = self;
        for p in parents {
            m = m.get_mut(p)?.as_map_mut()?;
        }
        m.remove(last)
    }
}

// ---- reading --------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

struct Line {
    no: usize,
    indent: usize,
    text: String,
}

/// Reads a document whose top level is a map (an empty file is an empty map).
pub fn parse(src: &str) -> Result<Map, Error> {
    let mut lines = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let trimmed = raw.trim_end();
        let content = trimmed.trim_start();
        if content.is_empty() || content.starts_with('#') || content == "---" {
            continue;
        }
        if raw.contains('\t') && raw[..raw.len() - raw.trim_start().len()].contains('\t') {
            return Err(Error {
                line: i + 1,
                message: "tab in indentation".into(),
            });
        }
        lines.push(Line {
            no: i + 1,
            indent: trimmed.len() - content.len(),
            text: content.to_owned(),
        });
    }
    let raw = src.lines().map(str::to_owned).collect();
    let mut p = Parser { lines, i: 0, raw };
    if p.lines.is_empty() {
        return Ok(Map::default());
    }
    // An empty document as Bukkit writes it: `{` / `}`.
    if p.lines[0].text.starts_with('{') {
        let first = p.lines[0].text.clone();
        let line = p.lines[0].no;
        p.i = 1;
        let node = p.flow(&first);
        return match node {
            Ok(Node::Map(m)) if m.entries.is_empty() && p.i == p.lines.len() => Ok(m),
            _ => Err(Error {
                line,
                message: "a top-level flow map must be empty".into(),
            }),
        };
    }
    let indent = p.lines[0].indent;
    let map = p.map(indent)?;
    if p.i < p.lines.len() {
        return Err(p.err("unexpected indentation"));
    }
    Ok(map)
}

struct Parser {
    lines: Vec<Line>,
    i: usize,
    /// The source lines as they are (block scalars keep blank and `#` lines).
    raw: Vec<String>,
}

impl Parser {
    fn err(&self, message: &str) -> Error {
        Error {
            line: self.lines.get(self.i).map_or(0, |l| l.no),
            message: message.into(),
        }
    }

    fn is_item(text: &str) -> bool {
        text == "-" || text.starts_with("- ")
    }

    fn block(&mut self, indent: usize) -> Result<Node, Error> {
        if Self::is_item(&self.lines[self.i].text) {
            Ok(Node::Seq(self.seq(indent)?))
        } else {
            Ok(Node::Map(self.map(indent)?))
        }
    }

    fn map(&mut self, indent: usize) -> Result<Map, Error> {
        let mut map = Map::default();
        while self.i < self.lines.len() {
            let line = &self.lines[self.i];
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(self.err("unexpected indentation in a map"));
            }
            if Self::is_item(&line.text) {
                break;
            }
            let (key, rest) = split_key(&line.text).ok_or_else(|| self.err("expected `key:`"))?;
            let rest = strip_comment(rest).trim().to_owned();
            self.i += 1;
            let value = self.value(indent, &rest, true)?;
            map.entries.push((key, value));
        }
        Ok(map)
    }

    /// The value after `key:` (or `- `): inline text, or the block on the next lines.
    fn value(&mut self, indent: usize, rest: &str, in_map: bool) -> Result<Node, Error> {
        if rest.is_empty() {
            if let Some(next) = self.lines.get(self.i) {
                if next.indent > indent {
                    return self.block(next.indent);
                }
                if in_map && next.indent == indent && Self::is_item(&next.text) {
                    return Ok(Node::Seq(self.seq(indent)?));
                }
            }
            return Ok(Node::Scalar(Scalar::null()));
        }
        if rest.starts_with('{') || rest.starts_with('[') {
            return self.flow(rest);
        }
        if rest.starts_with('|') || rest.starts_with('>') {
            return self.block_scalar(indent, rest);
        }
        Ok(Node::Scalar(Scalar { raw: self.continued_scalar(indent, rest)? }))
    }

    /// A plain or quoted scalar that SnakeYAML folded onto more-indented lines (width 80): the
    /// lines are joined with spaces, a blank line between them is a line break. Returns the raw
    /// scalar (one line, re-quoted when the text needs it).
    fn continued_scalar(&mut self, indent: usize, first: &str) -> Result<String, Error> {
        let quote = first.chars().next().filter(|c| *c == '\'' || *c == '"');
        let open = match quote {
            Some(q) => closing_quote(first, q).is_none(),
            None => false,
        };
        let mut parts = vec![first.to_owned()];
        let mut prev_no = self.lines.get(self.i.wrapping_sub(1)).map_or(0, |l| l.no);
        loop {
            let Some(next) = self.lines.get(self.i) else {
                if open {
                    return Err(self.err("unterminated quoted scalar"));
                }
                break;
            };
            let deeper = next.indent > indent;
            if !(open || (quote.is_none() && deeper)) {
                break;
            }
            // Blank lines in between are line breaks (the lexer dropped them).
            for _ in prev_no + 1..next.no {
                parts.push(String::new());
            }
            let text = if quote.is_none() { strip_comment(&next.text).trim().to_owned() } else { next.text.clone() };
            prev_no = next.no;
            self.i += 1;
            parts.push(text);
            if let Some(q) = quote {
                if closing_quote(&fold_lines(&parts), q).is_some() {
                    break;
                }
            }
        }
        if parts.len() == 1 {
            return Ok(first.to_owned());
        }
        let folded = fold_lines(&parts);
        Ok(match quote {
            // Re-read as one line: the quotes stay, the escapes inside too.
            Some(_) if !folded.contains('\n') => folded,
            Some(_) => quote_if_needed(&unquote(&folded)),
            None if folded.contains('\n') => quote_if_needed(&folded),
            None => folded,
        })
    }

    /// `|`, `|-`, `|+`, `>`, `>-`, `>+` (an indentation digit is accepted and ignored: the
    /// block's indent is that of its first line). Read from the raw source lines.
    fn block_scalar(&mut self, indent: usize, header: &str) -> Result<Node, Error> {
        let folded = header.starts_with('>');
        let chomp = if header.contains('-') {
            '-'
        } else if header.contains('+') {
            '+'
        } else {
            ' '
        };
        // The key's line: the block starts on the source line after it.
        let start = self.lines.get(self.i.wrapping_sub(1)).map_or(0, |l| l.no);
        let mut block_indent = None;
        let mut body: Vec<String> = Vec::new();
        let mut last_no = start;
        for (k, raw) in self.raw.iter().enumerate().skip(start) {
            let content = raw.trim_start();
            if content.trim().is_empty() {
                body.push(String::new());
                continue;
            }
            let ind = raw.len() - content.len();
            let bi = *block_indent.get_or_insert(ind);
            if ind < bi || ind <= indent {
                break;
            }
            body.push(raw[bi..].trim_end_matches('\r').to_owned());
            last_no = k + 1;
        }
        // Trailing blank lines after the last content line belong to the chomping only.
        let content_len = body.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
        let trailing = body.len() - content_len;
        body.truncate(content_len);
        let mut text = if folded { fold_block(&body) } else { body.join("\n") };
        match chomp {
            '-' => {}
            '+' => {
                if content_len > 0 {
                    text.push('\n');
                }
                text.extend(std::iter::repeat_n('\n', trailing));
            }
            _ => {
                if content_len > 0 {
                    text.push('\n');
                }
            }
        }
        while self.lines.get(self.i).is_some_and(|l| l.no <= last_no) {
            self.i += 1;
        }
        Ok(Node::Scalar(Scalar { raw: quote_if_needed(&text) }))
    }

    /// A flow collection: empty ones (also `{` with its `}` on a later line) and flat sequences of
    /// scalars.
    fn flow(&mut self, first: &str) -> Result<Node, Error> {
        let open = first.chars().next().unwrap_or('{');
        let close = if open == '{' { '}' } else { ']' };
        let mut text = first.to_owned();
        while !text.trim_end().ends_with(close) {
            let Some(next) = self.lines.get(self.i) else {
                return Err(self.err("unterminated flow collection"));
            };
            text.push(' ');
            text.push_str(&next.text);
            self.i += 1;
        }
        let inner = text.trim();
        let inner = inner[1..inner.len() - 1].trim();
        if inner.is_empty() {
            return Ok(if open == '{' { Node::map() } else { Node::Seq(Vec::new()) });
        }
        if open == '{' {
            return Err(self.err("non-empty flow maps are not supported"));
        }
        Ok(Node::Seq(
            split_flow(inner)
                .into_iter()
                .map(|raw| Node::Scalar(Scalar { raw }))
                .collect(),
        ))
    }

    fn seq(&mut self, indent: usize) -> Result<Vec<Node>, Error> {
        let mut items = Vec::new();
        while self.i < self.lines.len() {
            let line = &self.lines[self.i];
            if line.indent != indent || !Self::is_item(&line.text) {
                if line.indent > indent {
                    return Err(self.err("unexpected indentation in a sequence"));
                }
                break;
            }
            let content = line.text[1..].trim_start().to_owned();
            if content.is_empty() {
                self.i += 1;
                items.push(self.value(indent, "", false)?);
                continue;
            }
            if Self::is_item(&content) {
                // A compact nested sequence, `- - a`, as Bukkit writes a list of lists.
                let column = indent + (line.text.len() - content.len());
                self.lines[self.i].indent = column;
                self.lines[self.i].text = content;
                items.push(Node::Seq(self.seq(column)?));
                continue;
            }
            if split_key(&content).is_some() && !starts_quoted_scalar(&content) {
                // A map item: its first key sits after "- ", the others line up with it.
                let column = indent + (line.text.len() - content.len());
                self.lines[self.i].indent = column;
                self.lines[self.i].text = content;
                items.push(Node::Map(self.map(column)?));
                continue;
            }
            self.i += 1;
            let content = strip_comment(&content).trim().to_owned();
            items.push(self.value(indent, &content, false)?);
        }
        Ok(items)
    }
}

/// A quoted scalar that happens to contain `: ` is a scalar, not a key, unless a `:` follows its
/// closing quote.
fn starts_quoted_scalar(s: &str) -> bool {
    let Some(q) = s.chars().next().filter(|c| *c == '\'' || *c == '"') else {
        return false;
    };
    match closing_quote(s, q) {
        Some(end) => !s[end + 1..].trim_start().starts_with(':'),
        None => true,
    }
}

fn closing_quote(s: &str, q: char) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if q == '"' && c == '\\' {
            i += 2;
            continue;
        }
        if c == q {
            if q == '\'' && bytes.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Splits `key: rest` / `key:`; the key may be quoted.
fn split_key(text: &str) -> Option<(Key, &str)> {
    let first = text.chars().next()?;
    let end_key = if first == '\'' || first == '"' {
        let end = closing_quote(text, first)?;
        if !text[end + 1..].starts_with(':') {
            return None;
        }
        end + 1
    } else {
        let bytes = text.as_bytes();
        let mut found = None;
        for i in 0..bytes.len() {
            if bytes[i] == b':' && (i + 1 == bytes.len() || bytes[i + 1] == b' ') {
                found = Some(i);
                break;
            }
            if bytes[i] == b'#' && i > 0 && bytes[i - 1] == b' ' {
                return None;
            }
        }
        found?
    };
    let raw = text[..end_key].to_owned();
    let rest = text[end_key + 1..].strip_prefix(' ').unwrap_or(&text[end_key + 1..]);
    Some((
        Key {
            name: unquote(&raw),
            raw,
        },
        rest,
    ))
}

/// Drops a trailing ` # comment` from a plain scalar (quoted ones keep everything up to their
/// closing quote).
fn strip_comment(s: &str) -> &str {
    let t = s.trim_start();
    if let Some(q) = t.chars().next().filter(|c| *c == '\'' || *c == '"') {
        if let Some(end) = closing_quote(t, q) {
            return &t[..=end];
        }
        return t;
    }
    match t.find(" #") {
        Some(i) => &t[..i],
        None => t,
    }
}

fn split_flow(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match quote {
            Some(q) if c == q => {
                quote = None;
                cur.push(c);
            }
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                cur.push(c);
            }
            None if c == ',' => {
                out.push(cur.trim().to_owned());
                cur.clear();
            }
            None => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_owned());
    }
    out
}

/// The text of a raw scalar or key.
pub fn unquote(raw: &str) -> String {
    let r = raw.trim();
    if r.len() >= 2 && r.starts_with('\'') && r.ends_with('\'') {
        return r[1..r.len() - 1].replace("''", "'");
    }
    if r.len() >= 2 && r.starts_with('"') && r.ends_with('"') {
        let mut out = String::new();
        let mut chars = r[1..r.len() - 1].chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('0') => out.push('\0'),
                Some('x') => push_hex(&mut out, &mut chars, 2),
                Some('u') => push_hex(&mut out, &mut chars, 4),
                Some('U') => push_hex(&mut out, &mut chars, 8),
                Some(other) => out.push(other),
                None => {}
            }
        }
        return out;
    }
    r.to_owned()
}

fn push_hex(out: &mut String, chars: &mut std::str::Chars<'_>, n: usize) {
    let hex: String = chars.take(n).collect();
    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
        out.push(c);
    }
}

// ---- writing --------------------------------------------------------------------------------

/// Writes a document, optionally starting with comment lines (`header`, without `# `).
pub fn emit(map: &Map, header: &[&str]) -> String {
    let mut out = String::new();
    for h in header {
        let _ = writeln!(out, "# {h}");
    }
    if map.entries.is_empty() {
        out.push_str("{\n  }\n");
        return out;
    }
    emit_map(&mut out, map, 0);
    out
}

fn emit_map(out: &mut String, map: &Map, indent: usize) {
    for (key, value) in &map.entries {
        pad(out, indent);
        out.push_str(&key.raw);
        out.push(':');
        emit_value(out, value, indent, true);
    }
}

fn emit_value(out: &mut String, value: &Node, indent: usize, in_map: bool) {
    match value {
        Node::Scalar(s) => {
            if !s.raw.is_empty() {
                out.push(' ');
                out.push_str(&s.raw);
            }
            out.push('\n');
        }
        // SnakeYAML's pretty flow style, as Bukkit writes it: `{`, then `}` on the next line.
        Node::Map(m) if m.entries.is_empty() => {
            out.push_str(" {\n");
            pad(out, indent + 2);
            out.push_str("}\n");
        }
        Node::Map(m) => {
            out.push('\n');
            emit_map(out, m, indent + 2);
        }
        Node::Seq(s) if s.is_empty() => out.push_str(" []\n"),
        Node::Seq(s) => {
            out.push('\n');
            // Bukkit puts a map's sequence at the key's own indent.
            emit_seq(out, s, if in_map { indent } else { indent + 2 });
        }
    }
}

fn emit_seq(out: &mut String, seq: &[Node], indent: usize) {
    for item in seq {
        match item {
            Node::Map(m) if !m.entries.is_empty() => {
                let mut sub = String::new();
                emit_map(&mut sub, m, indent + 2);
                pad(out, indent);
                out.push_str("- ");
                out.push_str(&sub[indent + 2..]);
            }
            Node::Seq(inner) if !inner.is_empty() => {
                let mut sub = String::new();
                emit_seq(&mut sub, inner, indent + 2);
                pad(out, indent);
                out.push_str("- ");
                out.push_str(&sub[indent + 2..]);
            }
            other => {
                pad(out, indent);
                out.push('-');
                emit_value(out, other, indent, false);
            }
        }
    }
}

fn pad(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n(' ', n));
}

/// YAML line folding of a flow scalar's lines: one space between lines, a blank line is a line
/// break.
fn fold_lines(parts: &[String]) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for (n, p) in parts.iter().enumerate() {
        if p.is_empty() {
            out.push('\n');
            pending_space = false;
            continue;
        }
        if n > 0 && pending_space {
            out.push(' ');
        }
        out.push_str(p);
        pending_space = true;
    }
    out
}

/// A `>` block's body: lines folded with spaces, blank lines are line breaks, more-indented
/// lines are kept as they are.
fn fold_block(body: &[String]) -> String {
    let mut out = String::new();
    let mut prev_text = false;
    for line in body {
        if line.is_empty() {
            out.push('\n');
            prev_text = false;
        } else if line.starts_with(' ') {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
            prev_text = false;
        } else {
            if prev_text {
                out.push(' ');
            }
            out.push_str(line);
            prev_text = true;
        }
    }
    out
}

/// SnakeYAML's choice for a string: plain when it reads back as the same string, else single
/// quotes, else (line breaks, control characters) double quotes.
pub fn quote_if_needed(s: &str) -> String {
    if s.chars().any(|c| c.is_control()) {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                c if c.is_control() => {
                    let _ = write!(out, "\\x{:02X}", c as u32);
                }
                c => out.push(c),
            }
        }
        out.push('"');
        return out;
    }
    if needs_quotes(s) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        s.to_owned()
    }
}

fn needs_quotes(s: &str) -> bool {
    if s.is_empty() || s != s.trim() {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~" | "=" | "<<"
    ) {
        return true;
    }
    if looks_numeric(s) || looks_like_timestamp(s) {
        return true;
    }
    let first = s.chars().next().unwrap_or(' ');
    if "-?:,[]{}#&*!|>'\"%@`".contains(first) {
        return true;
    }
    s.contains(": ") || s.contains(" #") || s.ends_with(':')
}

fn looks_numeric(s: &str) -> bool {
    let t = s.trim_start_matches(['+', '-']);
    if t.is_empty() {
        return false;
    }
    if t.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return true;
    }
    s.parse::<f64>().is_ok()
        || matches!(t.to_ascii_lowercase().as_str(), ".inf" | ".nan")
        || t.starts_with("0x")
        || t.starts_with("0b")
        || t.starts_with("0o")
}

/// SnakeYAML's YAML 1.1 timestamp (`2026-11-06`, `2026-11-06 12:00:00`): read back as a Date.
fn looks_like_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| r.clone().all(|i| b.get(i).is_some_and(u8::is_ascii_digit));
    if b.len() < 8 || !digits(0..4) || b[4] != b'-' {
        return false;
    }
    // YYYY-M(M)-D(D), then optionally a time.
    let mut i = 5;
    let m = (i..b.len()).take_while(|&k| b[k].is_ascii_digit()).count();
    if !(1..=2).contains(&m) || b.get(i + m) != Some(&b'-') {
        return false;
    }
    i += m + 1;
    let d = (i..b.len()).take_while(|&k| b[k].is_ascii_digit()).count();
    if !(1..=2).contains(&d) {
        return false;
    }
    let rest = &s[i + d..];
    rest.is_empty() || rest.starts_with(['T', 't', ' ', '\t'])
}

/// A double as Java's `Double.toString` writes it (what Bukkit stores): `45.0`, `116.9647`,
/// `1.0E-4`, `1.2345678E7`.
pub fn java_double(f: f64) -> String {
    if !f.is_finite() {
        return if f.is_nan() {
            ".NaN".into()
        } else if f > 0.0 {
            ".inf".into()
        } else {
            "-.inf".into()
        };
    }
    let a = f.abs();
    if a == 0.0 || (1e-3..1e7).contains(&a) {
        let s = format!("{f:?}");
        if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
    } else {
        let s = format!("{f:e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_owned()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exp}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Citizens NPC Storage
last-created-npc-id: 7
npc:
  '3':
    metadata:
      protected: true
      nameplate-visible: 'False'
    traits:
      scoreboardtrait:
        color: WHITE
        tags:
        - CITIZENS_NPC
      lookclose:
        enabled: true
        randomYawRange:
        - 0.0
        - 360.0
      location:
        world: world
        x: 116.9647
        y: 45.0
      hologramtrait:
        lines:
        - renderer:
            billboard: CENTER
          margin:
            top: 0.0
          text: '&a&lOPEN'
        - text: plain line
      followtrait: {
        }
      commandtrait:
        commands:
          '1':
            command: say hi <p>
            hand: RIGHT
    traitnames: scoreboardtrait,spawned,location
    name: Guide
";

    #[test]
    fn reads_the_bukkit_layout() {
        let m = parse(SAMPLE).unwrap();
        assert_eq!(m.get("last-created-npc-id").unwrap().as_i64(), Some(7));
        let npc = m.get_path(&["npc", "3"]).unwrap().as_map().unwrap();
        assert_eq!(npc.get_path(&["metadata", "protected"]).unwrap().as_bool(), Some(true));
        assert_eq!(
            npc.get_path(&["metadata", "nameplate-visible"]).unwrap().as_bool(),
            Some(false)
        );
        let tags = npc.get_path(&["traits", "scoreboardtrait", "tags"]).unwrap();
        assert_eq!(tags.as_seq().unwrap()[0].text().as_deref(), Some("CITIZENS_NPC"));
        let lines = npc.get_path(&["traits", "hologramtrait", "lines"]).unwrap().as_seq().unwrap();
        assert_eq!(lines.len(), 2);
        let first = lines[0].as_map().unwrap();
        assert_eq!(first.get("text").unwrap().text().as_deref(), Some("&a&lOPEN"));
        assert_eq!(
            first.get_path(&["renderer", "billboard"]).unwrap().text().as_deref(),
            Some("CENTER")
        );
        assert_eq!(
            npc.get_path(&["traits", "followtrait"]).unwrap(),
            &Node::map()
        );
        assert_eq!(
            npc.get_path(&["traits", "commandtrait", "commands", "1", "command"])
                .unwrap()
                .text()
                .as_deref(),
            Some("say hi <p>")
        );
    }

    #[test]
    fn writes_back_what_it_read() {
        let m = parse(SAMPLE).unwrap();
        let out = emit(&m, &["Citizens NPC Storage"]);
        assert_eq!(out, SAMPLE);
        assert_eq!(parse(&out).unwrap(), m);
    }

    #[test]
    fn empty_documents_and_maps() {
        let m = parse("{\n  }\n").unwrap();
        assert!(m.entries.is_empty());
        assert_eq!(emit(&m, &["H"]), "# H\n{\n  }\n");
        let m = parse("a: 1\nnpc: {\n  }\n").unwrap();
        assert_eq!(emit(&m, &[]), "a: 1\nnpc: {\n  }\n");
        assert_eq!(parse("npc: {}\n").unwrap().get("npc"), Some(&Node::map()));
    }

    #[test]
    fn quotes_like_snakeyaml() {
        assert_eq!(quote_if_needed("3"), "'3'");
        assert_eq!(quote_if_needed("&a&lOPEN"), "'&a&lOPEN'");
        assert_eq!(quote_if_needed("False"), "'False'");
        assert_eq!(quote_if_needed("it's"), "it's");
        assert_eq!(quote_if_needed("a: b"), "'a: b'");
        assert_eq!(quote_if_needed(""), "''");
        assert_eq!(quote_if_needed("server lobby"), "server lobby");
        assert_eq!(quote_if_needed("line\nbreak"), "\"line\\nbreak\"");
        assert_eq!(unquote("'it''s'"), "it's");
        assert_eq!(unquote("\"a\\u00e9\""), "a\u{e9}");
    }

    #[test]
    fn doubles_like_java() {
        assert_eq!(java_double(45.0), "45.0");
        assert_eq!(java_double(116.9647), "116.9647");
        assert_eq!(java_double(0.0), "0.0");
        assert_eq!(java_double(-95.3283), "-95.3283");
        assert_eq!(java_double(0.0001), "1.0E-4");
        assert_eq!(java_double(12345678.0), "1.2345678E7");
    }

    #[test]
    fn edits_keep_order_and_spelling() {
        let mut m = parse("npc:\n  '3':\n    name: A\n").unwrap();
        m.set_path(&["npc", "3", "name"], Node::str("B"));
        m.set_path(&["npc", "4", "name"], Node::str("C"));
        assert_eq!(emit(&m, &[]), "npc:\n  '3':\n    name: B\n  '4':\n    name: C\n");
        assert!(m.remove_path(&["npc", "3"]).is_some());
        assert_eq!(emit(&m, &[]), "npc:\n  '4':\n    name: C\n");
    }

    #[test]
    fn nested_sequences_and_flow_lists() {
        let m = parse("a:\n  - x\n  -\n    - y\n    - z\nb: [1, 'two', 3]\nc: []\n").unwrap();
        let a = m.get("a").unwrap().as_seq().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].as_seq().unwrap().len(), 2);
        let b = m.get("b").unwrap().as_seq().unwrap();
        assert_eq!(b[1].text().as_deref(), Some("two"));
        assert_eq!(m.get("c").unwrap(), &Node::Seq(vec![]));
    }

    // ---- review C1, C2, C6 (blitz) ----------------------------------------------------------

    #[test]
    fn folded_plain_and_quoted_scalars() {
        let src = "a:\n  b:\n    command: say Welcome to the server, please read the rules and have a lot\n      of fun with everyone here <p>\n    longquoted: '&a&lA very long hologram line with colour codes that goes past\n      eighty characters for sure'\n    after: 1\n";
        let m = parse(src).expect("folded scalars read");
        let b = m.get("a").unwrap().as_map().unwrap().get("b").unwrap().as_map().unwrap();
        assert_eq!(
            b.get("command").unwrap().text().as_deref(),
            Some("say Welcome to the server, please read the rules and have a lot of fun with everyone here <p>")
        );
        assert_eq!(
            b.get("longquoted").unwrap().text().as_deref(),
            Some("&a&lA very long hologram line with colour codes that goes past eighty characters for sure")
        );
        assert_eq!(b.get("after").unwrap().text().as_deref(), Some("1"));
        // Written back on one line, read back the same.
        let again = parse(&emit(&m, &[])).unwrap();
        assert_eq!(again, m);
    }

    #[test]
    fn folded_plain_in_a_sequence_item() {
        let src = "lines:\n- first item that is long enough to be folded by the writer at eighty\n  columns wide\n- second\n";
        let m = parse(src).unwrap();
        let s = m.get("lines").unwrap().as_seq().unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0].text().as_deref(),
            Some("first item that is long enough to be folded by the writer at eighty columns wide")
        );
    }

    #[test]
    fn literal_and_folded_blocks() {
        let src = "h:\n  lines:\n    - text: |-\n        line one\n        line two\n      height: 0.25\n  keep: |\n    a\n\n    # not a comment\n  fold: >-\n    one\n    two\n\n    three\n  last: x\n";
        let m = parse(src).expect("block scalars read");
        let h = m.get("h").unwrap().as_map().unwrap();
        let item = h.get("lines").unwrap().as_seq().unwrap()[0].as_map().unwrap();
        assert_eq!(item.get("text").unwrap().text().as_deref(), Some("line one\nline two"));
        assert_eq!(item.get("height").unwrap().text().as_deref(), Some("0.25"));
        assert_eq!(h.get("keep").unwrap().text().as_deref(), Some("a\n\n# not a comment\n"));
        assert_eq!(h.get("fold").unwrap().text().as_deref(), Some("one two\nthree"));
        assert_eq!(h.get("last").unwrap().text().as_deref(), Some("x"));
        let again = parse(&emit(&m, &[])).unwrap();
        assert_eq!(again.get("h").unwrap().as_map().unwrap().get("fold").unwrap().text().as_deref(), Some("one two\nthree"));
    }

    #[test]
    fn compact_nested_sequences() {
        let src = "grid:\n- - a\n  - b\n- - c\n";
        let m = parse(src).expect("nested sequences read");
        let g = m.get("grid").unwrap().as_seq().unwrap();
        assert_eq!(g.len(), 2);
        let first = g[0].as_seq().unwrap();
        assert_eq!(first[1].text().as_deref(), Some("b"));
        assert_eq!(g[1].as_seq().unwrap()[0].text().as_deref(), Some("c"));
        assert_eq!(emit(&m, &[]), src, "written as Bukkit writes it");
    }

    #[test]
    fn dates_and_other_typed_forms_are_quoted() {
        assert_eq!(quote_if_needed("2026-11-06"), "'2026-11-06'");
        assert_eq!(quote_if_needed("2026-1-6 10:00"), "'2026-1-6 10:00'");
        assert_eq!(quote_if_needed("0b101"), "'0b101'");
        assert_eq!(quote_if_needed("0o17"), "'0o17'");
        assert_eq!(quote_if_needed("1_000"), "'1_000'");
        assert_eq!(quote_if_needed("<<"), "'<<'");
        assert_eq!(quote_if_needed("y"), "y", "SnakeYAML reads y as a string");
        assert_eq!(quote_if_needed("2026-11-06x"), "2026-11-06x");
    }
}

