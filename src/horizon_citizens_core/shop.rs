//! NPC shops (`ShopTrait`, `trait/shop/*`): the stored shops in Citizens' `shops.yml` format, the
//! actions (item, permission, money, command, experience, open_shop, condition), the purchase
//! rules of `NPCShopItem.onClick`, and a small Molang subset for the condition expressions.
//!
//! Nothing here touches the game. A purchase is planned against a copy of what the buyer holds
//! ([`Buyer`]) and answers with the new levels and inventory plus the effects to run ([`Outcome`]);
//! commands and open-shop actions run when their step is reached, as in the original (they cannot
//! be rolled back there either).
//!
//! Save format (`PersistenceLoader` of Citizens): `global.<name>` and `npc.<npc uuid>` each hold
//! `pages.<n>.items.<slot>`, `storage`, `title`, `type`, `viewPermission`; a list is a map keyed by
//! its index, an action is its fields plus `type` (`items`, `permissions`, `money`, `command`,
//! `experience`, `open_shop`, `condition`).

use crate::horizon_citizens_core::npc::StackData;
use crate::horizon_citizens_core::tree::TreeExt;
use crate::horizon_citizens_core::yaml::{self, Document, Node};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const HEADER: &str = "Citizens NPC Shops";

/// `Setting.SHOP_*` (`npc.shops.*`).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ShopsConfig {
    /// `npc.shops.global-view-permission`: a node every viewer needs (empty: none).
    pub global_view_permission: String,
    /// `npc.shops.add-default-item-description`.
    pub add_default_item_description: bool,
    /// `npc.shops.default-item`.
    pub default_item: DefaultItem,
}

/// `npc.shops.default-item`: what a new shop item starts with (all empty by default).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct DefaultItem {
    pub already_purchased_message: String,
    pub click_to_confirm_message: String,
    pub cost_message: String,
    pub result_message: String,
    pub max_repeats_on_shift_click: bool,
    pub times_purchasable: i32,
    pub global_times_purchasable: i32,
    pub lore: String,
    pub name: String,
}

/// Every node the shop commands and editors ask (besides the per-kind action nodes of [`Kind::edit_node`]).
pub const NODES: &[&str] = &[
    "citizens.npc.shop",
    "citizens.npc.shopitem",
    "citizens.npc.showshop",
    "citizens.npc.showshop.to-others",
    "citizens.npc.shop.edit",
    "citizens.npc.shop.editor.set-shop-type",
    "citizens.npc.shop.editor.show-on-right-click",
];

// ---- shop type ----------------------------------------------------------------------------

/// `ShopTrait.ShopType`: the size of the chest and the cells of the page buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShopType {
    Chest1x9,
    Chest2x9,
    Chest3x9,
    Chest4x9,
    Default,
    Trader,
}

impl ShopType {
    /// In the order the type picker lists them.
    pub const ALL: [ShopType; 6] =
        [ShopType::Default, ShopType::Chest4x9, ShopType::Chest3x9, ShopType::Chest2x9, ShopType::Chest1x9, ShopType::Trader];

    /// The Java enum name, as saved.
    pub fn name(self) -> &'static str {
        match self {
            ShopType::Chest1x9 => "CHEST_1X9",
            ShopType::Chest2x9 => "CHEST_2X9",
            ShopType::Chest3x9 => "CHEST_3X9",
            ShopType::Chest4x9 => "CHEST_4X9",
            ShopType::Default => "DEFAULT",
            ShopType::Trader => "TRADER",
        }
    }

    pub fn parse(s: &str) -> Option<ShopType> {
        ShopType::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(s.trim()))
    }

    /// `inventorySize`.
    pub fn size(self) -> usize {
        match self {
            ShopType::Chest1x9 => 9,
            ShopType::Chest2x9 => 18,
            ShopType::Chest3x9 => 27,
            ShopType::Chest4x9 => 36,
            ShopType::Default | ShopType::Trader => 45,
        }
    }

    pub fn rows(self) -> u8 {
        (self.size() / 9) as u8
    }

    /// `prevSlotIndex` (`CHEST_1X9` passes 7, 6, 8; the others `size - 9 + 3, +4, +5`).
    pub fn prev_slot(self) -> usize {
        if self == ShopType::Chest1x9 { 7 } else { self.size() - 9 + 3 }
    }

    pub fn edit_slot(self) -> usize {
        if self == ShopType::Chest1x9 { 6 } else { self.size() - 9 + 4 }
    }

    pub fn next_slot(self) -> usize {
        if self == ShopType::Chest1x9 { 8 } else { self.size() - 9 + 5 }
    }
}

// ---- actions ------------------------------------------------------------------------------

/// `NPCShopAction` and its seven kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Item { items: Vec<StackData>, meta_filter: Vec<String>, require_undamaged: bool },
    Permission { permissions: Vec<String> },
    Money { money: f64 },
    Command { commands: Vec<String>, op: bool, server: bool },
    Experience { exp: i32 },
    OpenShop { shop_name: Option<String> },
    Condition { condition: Option<String> },
}

/// The kinds in the order `ShopTrait` registers their editors: the editor lists them in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Item,
    Permission,
    Money,
    Command,
    Experience,
    OpenShop,
    Condition,
}

impl Kind {
    pub const ALL: [Kind; 7] =
        [Kind::Item, Kind::Permission, Kind::Money, Kind::Command, Kind::Experience, Kind::OpenShop, Kind::Condition];

    /// The registry name (`NPCShopAction.register`).
    pub fn type_name(self) -> &'static str {
        match self {
            Kind::Item => "items",
            Kind::Permission => "permissions",
            Kind::Money => "money",
            Kind::Command => "command",
            Kind::Experience => "experience",
            Kind::OpenShop => "open_shop",
            Kind::Condition => "condition",
        }
    }

    /// `GUI.canUse`: the node the editor of this kind needs.
    pub fn edit_node(self) -> &'static str {
        match self {
            Kind::Item => "citizens.npc.shop.editor.actions.edit-item",
            Kind::Permission => "citizens.npc.shop.editor.actions.edit-permission",
            Kind::Money => "citizens.npc.shop.editor.actions.edit-money",
            Kind::Command => "citizens.npc.shop.editor.actions.edit-command",
            Kind::Experience => "citizens.npc.shop.editor.actions.edit-exp",
            Kind::OpenShop => "citizens.npc.shop.editor.actions.edit-open-shop",
            Kind::Condition => "citizens.npc.shop.editor.actions.edit-condition",
        }
    }

    /// `GUI.createMenuItem`: the editor button's item and name. Money is offered only with an economy
    /// (see [`Kind::money_button`]).
    pub fn button(self) -> Option<(&'static str, &'static str)> {
        match self {
            Kind::Item => Some(("minecraft:chest", "Item")),
            Kind::Permission => Some(("minecraft:paper", "Permission")),
            Kind::Money => None,
            Kind::Command => Some(("minecraft:command_block", "Command")),
            Kind::Experience => Some(("minecraft:experience_bottle", "XP Level")),
            Kind::OpenShop => Some(("minecraft:bookshelf", "Open Shop")),
            Kind::Condition => Some(("minecraft:enchanted_book", "Condition")),
        }
    }
}

impl Kind {
    /// The money button, shown when an economy provider answers (`MoneyActionGUI.createMenuItem`).
    pub const fn money_button() -> (&'static str, &'static str) {
        ("minecraft:gold_ingot", "Money")
    }
}

/// Everything the actions ask the rest of the world about other shops.
pub trait Lookup {
    /// The stored shop `name` (a number is an NPC id): its canonical name, when it exists.
    fn shop_name(&self, name: &str) -> Option<String>;
    /// `NPCShop.canView` for the buyer.
    fn can_view(&self, name: &str, buyer: &Buyer) -> bool;
    /// `Economy.format`: an amount with the economy's currency (a plain double without one).
    fn format_money(&self, amount: f64) -> String {
        java_double(amount)
    }
}

/// What a buyer has: the experience level, the 36 main-inventory slots, and the permission nodes known to be held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Buyer {
    pub uuid: u128,
    pub name: String,
    pub world: String,
    pub levels: i32,
    pub inv: Vec<Option<StackData>>,
    pub perms: BTreeSet<String>,
    /// Whether the player is an operator (an undeclared node is held by operators, as in Bukkit).
    pub op: bool,
    /// The player's balance in the economy (0 without one).
    pub money: f64,
}

/// Something a purchase does outside the buyer's levels and inventory.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// A command as the console runs it, or as the player (`execute as`).
    Command { line: String, as_player: bool },
    /// Close the menu and show this shop a tick later.
    OpenShop(String),
    /// `Permission.playerAdd` (`set`) or `playerRemove`.
    Perm { node: String, set: bool },
}

/// The result of a click.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Lines for the player (legacy markup, `sendColorless`).
    pub messages: Vec<String>,
    pub effects: Vec<Effect>,
    /// Whether the purchase went through (levels and inventory below are then the new state).
    pub done: bool,
    pub levels: i32,
    pub inv: Vec<Option<StackData>>,
    /// The buyer's balance after the purchase.
    pub money: f64,
}

/// The working copy a purchase runs on.
#[derive(Clone)]
struct Work {
    levels: i32,
    /// The buyer's balance, and the shop storage's.
    money: f64,
    balance: f64,
    inv: Vec<Option<StackData>>,
    perms: BTreeSet<String>,
    /// The shop's storage inventory (limited storage only).
    storage: Vec<Option<StackData>>,
}

fn similar(a: &StackData, b: &StackData) -> bool {
    a.id == b.id
        && a.meta.display_name == b.meta.display_name
        && a.meta.lore == b.meta.lore
        && a.meta.enchants == b.meta.enchants
        && a.meta.damage.unwrap_or(0) == b.meta.damage.unwrap_or(0)
}

/// The largest stack of an item: tools, armour and the like 1, thrown items and signs 16, else 64
/// (`max-stack-size` in the meta wins). The game's item table is not reachable from here.
pub fn max_stack(s: &StackData) -> i64 {
    if let Some(m) = s.meta.max_stack_size {
        return i64::from(m.max(1));
    }
    let n = s.id.trim_start_matches("minecraft:");
    let one = ["_sword", "_pickaxe", "_axe", "_shovel", "_hoe", "_helmet", "_chestplate", "_leggings", "_boots", "_spear", "_horse_armor", "_boat", "_raft"];
    if one.iter().any(|e| n.ends_with(e))
        || matches!(
            n,
            "bow" | "crossbow" | "shield" | "elytra" | "trident" | "mace" | "fishing_rod" | "flint_and_steel" | "shears" | "totem_of_undying"
                | "potion" | "splash_potion" | "lingering_potion" | "enchanted_book" | "water_bucket" | "lava_bucket" | "milk_bucket"
                | "saddle" | "carrot_on_a_stick" | "warped_fungus_on_a_stick" | "brush" | "minecart"
        )
        || (n.ends_with("_bucket") && n != "bucket")
        || (n.ends_with("_minecart"))
    {
        return 1;
    }
    if matches!(n, "ender_pearl" | "egg" | "snowball" | "bucket" | "honey_bottle" | "armor_stand" | "name_tag")
        || n.ends_with("_sign")
        || n.ends_with("_banner")
    {
        return 16;
    }
    64
}

/// `ItemAction.tooDamaged`.
fn too_damaged(a: &Action, s: &StackData) -> bool {
    match a {
        Action::Item { require_undamaged, .. } => *require_undamaged && s.meta.damage.unwrap_or(0) != 0,
        _ => false,
    }
}

/// The value of a component of the stack, by its first path segment (the components the port maps).
fn component_text(s: &StackData, key: &str) -> Option<String> {
    let k = key.trim_start_matches("minecraft:");
    match k {
        "custom_name" => s.meta.display_name.clone(),
        "lore" => (!s.meta.lore.is_empty()).then(|| s.meta.lore.join("\n")),
        "damage" => s.meta.damage.map(|d| d.to_string()),
        "repair_cost" => s.meta.repair_cost.map(|d| d.to_string()),
        "max_stack_size" => s.meta.max_stack_size.map(|d| d.to_string()),
        "potion_contents" => s.meta.potion.clone(),
        "enchantments" => {
            if s.meta.enchants.is_empty() {
                None
            } else {
                let mut e: Vec<String> = s.meta.enchants.iter().map(|(i, l)| format!("{i}={l}")).collect();
                e.sort();
                Some(e.join(","))
            }
        }
        _ => None,
    }
}

/// `ItemAction.metaMatches`: every filter path must exist in the shop's stack and be equal in the player's.
fn meta_matches(needle: &StackData, hay: &StackData, filter: &[String]) -> bool {
    for path in filter {
        let first = path.split('.').next().unwrap_or("");
        let Some(want) = component_text(needle, first) else { return false };
        let Some(have) = component_text(hay, first) else { return false };
        if want != have {
            return false;
        }
    }
    true
}

impl Action {
    pub fn kind(&self) -> Kind {
        match self {
            Action::Item { .. } => Kind::Item,
            Action::Permission { .. } => Kind::Permission,
            Action::Money { .. } => Kind::Money,
            Action::Command { .. } => Kind::Command,
            Action::Experience { .. } => Kind::Experience,
            Action::OpenShop { .. } => Kind::OpenShop,
            Action::Condition { .. } => Kind::Condition,
        }
    }

    /// A fresh action (the no-argument constructors).
    pub fn new(kind: Kind) -> Action {
        match kind {
            Kind::Item => Action::Item { items: Vec::new(), meta_filter: Vec::new(), require_undamaged: true },
            Kind::Permission => Action::Permission { permissions: Vec::new() },
            Kind::Money => Action::Money { money: 0.0 },
            Kind::Command => Action::Command { commands: Vec::new(), op: false, server: false },
            Kind::Experience => Action::Experience { exp: 0 },
            Kind::OpenShop => Action::OpenShop { shop_name: None },
            Kind::Condition => Action::Condition { condition: None },
        }
    }

    /// `describe()`: the text of the default description and the editor buttons.
    pub fn describe(&self, shops: &dyn Lookup) -> String {
        match self {
            Action::Item { items, .. } => {
                if items.len() == 1 {
                    return stringify(&items[0]);
                }
                let mut d = format!("{} items", items.len());
                for (i, it) in items.iter().enumerate() {
                    d.push('\n');
                    d.push_str(&stringify(it));
                    if i == 3 {
                        d.push_str("...");
                        break;
                    }
                }
                d
            }
            Action::Permission { permissions } => list_description(permissions, &format!("{} permissions", permissions.len())),
            Action::Command { commands, .. } => list_description(commands, &format!("{} command", commands.len())),
            Action::Money { money } => shops.format_money(*money),
            Action::Experience { exp } => {
                if *exp == 1 { format!("{exp} level") } else { format!("{exp} levels") }
            }
            Action::OpenShop { shop_name } => match shop_name.as_deref().and_then(|n| shops.shop_name(n)) {
                Some(n) => format!("Open {n}"),
                None => "Open shop (currently unset)".to_owned(),
            },
            Action::Condition { condition } => condition.clone().unwrap_or_default(),
        }
    }

    /// `getMaxRepeats`: -1 for no limit.
    fn max_repeats(&self, w: &Work) -> i64 {
        match self {
            Action::Item { items, .. } => {
                if items.is_empty() {
                    return 0;
                }
                let mut has = vec![0i64; items.len()];
                for s in w.inv.iter().flatten() {
                    if too_damaged(self, s) {
                        continue;
                    }
                    for (j, it) in items.iter().enumerate() {
                        if self.matches(it, s) {
                            has[j] += s.count;
                            break;
                        }
                    }
                }
                let mut best = i64::MAX;
                for (j, it) in items.iter().enumerate() {
                    let r = if it.count == 0 { 0 } else { has[j] / it.count };
                    best = best.min(r);
                }
                best
            }
            Action::Experience { exp } => {
                if *exp == 0 { -1 } else { i64::from(w.levels / exp) }
            }
            Action::Money { money } => {
                if *money <= 0.0 { -1 } else { (w.money / money).floor() as i64 }
            }
            _ => -1,
        }
    }

    /// `ItemAction.matches`.
    fn matches(&self, a: &StackData, b: &StackData) -> bool {
        let Action::Item { meta_filter, .. } = self else { return false };
        if a.id != b.id {
            return false;
        }
        if !meta_filter.is_empty() {
            return meta_matches(a, b, meta_filter);
        }
        similar(a, b)
    }

    /// `ItemAction.takeItems`: whether `repeats` times the items are in `contents`; with `modify` they are removed.
    fn take_items(&self, contents: &mut [Option<StackData>], repeats: i64, modify: bool) -> bool {
        let Action::Item { items, .. } = self else { return false };
        let mut req: Vec<i64> = items.iter().map(|i| i.count * repeats).collect();
        for i in 0..contents.len() {
            let Some(cur) = contents[i].clone() else { continue };
            if too_damaged(self, &cur) {
                continue;
            }
            let mut to_match = Some(cur);
            for (j, item) in items.iter().enumerate() {
                let Some(tm) = to_match.clone() else { break };
                if req[j] <= 0 || !self.matches(item, &tm) {
                    continue;
                }
                let remaining = req[j];
                let taken = if tm.count > remaining { remaining } else { tm.count };
                if tm.count == taken {
                    to_match = None;
                } else {
                    let mut left = tm;
                    left.count -= taken;
                    to_match = Some(left);
                }
                if modify {
                    contents[i] = to_match.clone();
                }
                req[j] = remaining - taken;
            }
        }
        req.iter().sum::<i64>() <= 0
    }

    /// `ItemAction.giveItems`, kept as written: it returns as soon as everything of one repeat is placed while
    /// a slot is still left to look at, so a later repeat can be dropped.
    fn give_items(&self, inv: &mut [Option<StackData>], repeats: i64) {
        let Action::Item { items, .. } = self else { return };
        for _ in 0..repeats {
            let mut to_add: Vec<StackData> = items.clone();
            for j in 0..inv.len() {
                if to_add.is_empty() {
                    return;
                }
                let Some(slot) = inv[j].as_mut() else { continue };
                let Some(last) = to_add.last_mut() else { continue };
                let max = max_stack(slot);
                if !similar(slot, last) || slot.count >= max {
                    continue;
                }
                let diff = max - slot.count;
                if diff >= last.count {
                    slot.count += last.count;
                    to_add.pop();
                } else {
                    slot.count += diff;
                    last.count -= diff;
                }
            }
            for j in 0..inv.len() {
                if to_add.is_empty() {
                    break;
                }
                if inv[j].is_none() {
                    inv[j] = to_add.pop();
                }
            }
        }
    }

    fn can_accept(&self, inv: &[Option<StackData>], repeats: i64) -> bool {
        let Action::Item { items, .. } = self else { return false };
        let free = inv.iter().filter(|s| s.is_none()).count() as i64;
        free >= items.len() as i64 * repeats
    }
}

fn list_description(list: &[String], head: &str) -> String {
    let mut d = head.to_owned();
    for (i, l) in list.iter().enumerate() {
        d.push('\n');
        d.push_str(l);
        if i == 3 {
            d.push_str("...");
            break;
        }
    }
    d
}

/// `{amount} {pretty name}`.
pub fn stringify(s: &StackData) -> String {
    format!("{} {}", s.count, pretty(&s.id))
}

/// `Util.prettyEnum`: `diamond_sword` is `Diamond Sword`.
pub fn pretty(id: &str) -> String {
    id.trim_start_matches("minecraft:")
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Java's `Double.toString` for the plain values (`5.0`, `0.25`).
pub fn java_double(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e7 { format!("{v:.1}") } else { format!("{v}") }
}

// ---- storage, items, pages, shops ----------------------------------------------------------

/// `NPCShopStorage`.
#[derive(Clone, Debug, PartialEq)]
pub struct Storage {
    pub balance: f64,
    pub inventory: Vec<StackData>,
    pub inventory_size_limit: i32,
    pub unlimited: bool,
}

impl Default for Storage {
    fn default() -> Self {
        Storage { balance: 0.0, inventory: Vec::new(), inventory_size_limit: -1, unlimited: true }
    }
}

impl Storage {
    /// `canAdd`.
    pub fn can_add(&self, n: i64) -> bool {
        self.inventory_size_limit == -1 || self.inventory.len() as i64 + n < i64::from(self.inventory_size_limit)
    }

    pub fn from_node(n: &Node) -> Storage {
        let mut s = Storage::default();
        if let Some(b) = n.get("balance").and_then(Node::as_f64) {
            s.balance = b;
        }
        if let Some(inv) = n.get("inventory") {
            s.inventory = listed(inv).into_iter().filter_map(StackData::from_node).collect();
        }
        if let Some(l) = n.get("inventorySizeLimit").and_then(Node::as_i64) {
            s.inventory_size_limit = l as i32;
        }
        if let Some(u) = n.get("unlimited").and_then(Node::as_bool) {
            s.unlimited = u;
        }
        s
    }

    pub fn to_node(&self) -> Node {
        let mut n = Node::map();
        n.set("balance", Node::float(self.balance));
        if !self.inventory.is_empty() {
            n.set("inventory", indexed(self.inventory.iter().map(StackData::to_node).collect()));
        }
        n.set("inventorySizeLimit", Node::int(i64::from(self.inventory_size_limit)));
        n.set("unlimited", Node::bool(self.unlimited));
        n
    }
}

/// A list the way `PersistenceLoader` saves it: a map keyed by the index.
fn indexed(items: Vec<Node>) -> Node {
    let mut m = Node::map();
    for (i, it) in items.into_iter().enumerate() {
        m.set(&i.to_string(), it);
    }
    m
}

/// A saved list back: the values in index order (numeric keys), a real sequence as it is.
fn listed(n: &Node) -> Vec<&Node> {
    if n.is_seq() {
        return n.items().iter().collect();
    }
    let mut v: Vec<(i64, &Node)> =
        n.entries().iter().enumerate().map(|(i, e)| (e.key.trim().parse().unwrap_or(i as i64), &e.value)).collect();
    v.sort_by_key(|(k, _)| *k);
    v.into_iter().map(|(_, n)| n).collect()
}

fn text_of(n: &Node, key: &str) -> Option<String> {
    n.get(key).filter(|v| !v.is_null()).and_then(|v| v.text())
}

fn strings_of(n: &Node, key: &str) -> Vec<String> {
    n.get(key).map(|l| listed(l).into_iter().filter_map(|v| v.text()).collect()).unwrap_or_default()
}

fn set_text(n: &mut Node, key: &str, v: &Option<String>) {
    if let Some(v) = v {
        n.set(key, Node::str(v));
    }
}

impl Action {
    pub fn from_node(n: &Node) -> Option<Action> {
        let ty = text_of(n, "type")?;
        Some(match ty.as_str() {
            "items" => Action::Item {
                items: n.get("items").map(|l| listed(l).into_iter().filter_map(StackData::from_node).collect()).unwrap_or_default(),
                meta_filter: strings_of(n, "metaFilter"),
                require_undamaged: n.get("requireUndamaged").and_then(Node::as_bool).unwrap_or(true),
            },
            "permissions" => Action::Permission { permissions: strings_of(n, "permissions") },
            "money" => Action::Money { money: n.get("money").and_then(Node::as_f64).unwrap_or(0.0) },
            "command" => Action::Command {
                commands: strings_of(n, "commands"),
                op: n.get("op").and_then(Node::as_bool).unwrap_or(false),
                server: n.get("server").and_then(Node::as_bool).unwrap_or(false),
            },
            "experience" => Action::Experience { exp: n.get("exp").and_then(Node::as_i64).unwrap_or(0) as i32 },
            "open_shop" => Action::OpenShop { shop_name: text_of(n, "shopName") },
            "condition" => Action::Condition { condition: text_of(n, "condition") },
            _ => return None,
        })
    }

    pub fn to_node(&self) -> Node {
        let mut n = Node::map();
        let strs = |v: &[String]| indexed(v.iter().map(|s| Node::str(s)).collect());
        match self {
            Action::Item { items, meta_filter, require_undamaged } => {
                if !items.is_empty() {
                    n.set("items", indexed(items.iter().map(StackData::to_node).collect()));
                }
                if !meta_filter.is_empty() {
                    n.set("metaFilter", strs(meta_filter));
                }
                n.set("requireUndamaged", Node::bool(*require_undamaged));
            }
            Action::Permission { permissions } => {
                if !permissions.is_empty() {
                    n.set("permissions", strs(permissions));
                }
            }
            Action::Money { money } => n.set("money", Node::float(*money)),
            Action::Command { commands, op, server } => {
                if !commands.is_empty() {
                    n.set("commands", strs(commands));
                }
                n.set("op", Node::bool(*op));
                n.set("server", Node::bool(*server));
            }
            Action::Experience { exp } => n.set("exp", Node::int(i64::from(*exp))),
            Action::OpenShop { shop_name } => set_text(&mut n, "shopName", shop_name),
            Action::Condition { condition } => set_text(&mut n, "condition", condition),
        }
        n.set("type", Node::str(self.kind().type_name()));
        n
    }
}

/// `NPCShopItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShopItem {
    pub already_purchased_message: Option<String>,
    pub click_to_confirm_message: Option<String>,
    pub cost: Vec<Action>,
    pub cost_message: Option<String>,
    pub display: Option<StackData>,
    pub global_times_purchasable: i32,
    pub is_clickable_expression: Option<String>,
    pub is_visible_expression: Option<String>,
    pub max_repeats_on_shift_click: bool,
    pub npurchases: i32,
    pub purchases: BTreeMap<u128, i32>,
    pub result: Vec<Action>,
    pub result_message: Option<String>,
    pub times_purchasable: i32,
    default_lore: Vec<String>,
    default_name: Option<String>,
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() { None } else { Some(s.to_owned()) }
}

impl ShopItem {
    /// The constructor: defaults from `npc.shops.default-item`.
    pub fn new(cfg: &ShopsConfig) -> ShopItem {
        let d = &cfg.default_item;
        ShopItem {
            already_purchased_message: non_empty(&d.already_purchased_message),
            click_to_confirm_message: non_empty(&d.click_to_confirm_message),
            cost: Vec::new(),
            cost_message: non_empty(&d.cost_message),
            display: None,
            global_times_purchasable: d.global_times_purchasable,
            is_clickable_expression: None,
            is_visible_expression: None,
            max_repeats_on_shift_click: d.max_repeats_on_shift_click,
            npurchases: 0,
            purchases: BTreeMap::new(),
            result: Vec::new(),
            result_message: non_empty(&d.result_message),
            times_purchasable: d.times_purchasable,
            default_lore: if d.lore.is_empty() { Vec::new() } else { d.lore.split("<br>").map(str::to_owned).collect() },
            default_name: non_empty(&d.name),
        }
    }

    /// `setDisplayItem`: a copy, named and lored from the defaults.
    pub fn set_display_item(&mut self, s: Option<StackData>) {
        self.display = s;
        let Some(d) = self.display.as_mut() else { return };
        if let Some(name) = &self.default_name {
            d.meta.display_name = Some(name.replace("<itemname>", &pretty(&d.id)));
        }
        if !self.default_lore.is_empty() {
            let mut out = Vec::new();
            for l in &self.default_lore {
                if l.trim() == "<itemlore>" {
                    out.extend(d.meta.lore.clone());
                } else {
                    out.push(l.clone());
                }
            }
            d.meta.lore = out;
        }
    }

    pub fn reset_purchase_history(&mut self) {
        self.purchases.clear();
    }

    pub fn reset_player_history(&mut self, player: u128) {
        self.purchases.remove(&player);
    }

    /// `placeholders`: the player's placeholders, then `<cost>`, `<result>` and `<times_purchasable>`.
    pub fn placeholders(&self, s: &str, buyer: Option<&Buyer>, shops: &dyn Lookup) -> String {
        let s = match buyer {
            Some(b) => player_placeholders(s, b),
            None => s.to_owned(),
        };
        let mut out = String::new();
        let mut rest = s.as_str();
        while let Some(i) = rest.find('<') {
            let after = &rest[i + 1..];
            let Some(j) = after.find('>') else { break };
            let tag = after[..j].to_ascii_lowercase();
            if tag == "times_purchasable" || tag == "cost" || tag == "result" {
                out.push_str(&rest[..i]);
                match tag.as_str() {
                    "times_purchasable" => out.push_str(&self.times_purchasable.to_string()),
                    t => {
                        let list = if t == "cost" { &self.cost } else { &self.result };
                        out.push_str(&list.iter().map(|a| a.describe(shops)).collect::<Vec<_>>().join(", "));
                    }
                }
                rest = &after[j + 1..];
            } else {
                out.push_str(&rest[..=i]);
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }

    /// `getDisplayItem(player)`: None when there is none or the visible expression hides it.
    pub fn display_item(&self, buyer: Option<&Buyer>, cfg: &ShopsConfig, shops: &dyn Lookup) -> Option<StackData> {
        let mut stack = self.display.clone()?;
        if let Some(expr) = &self.is_visible_expression
            && !eval_pass(expr, buyer)
        {
            return None;
        }
        if let Some(name) = stack.meta.display_name.clone() {
            let replaced = self.placeholders(&name, buyer, shops);
            if replaced != name {
                stack.meta.display_name = Some(replaced);
            }
        }
        if cfg.add_default_item_description && stack.meta.lore.is_empty() {
            let mut lore: Vec<String> = Vec::new();
            for c in &self.cost {
                lore.extend(c.describe(shops).split('\n').map(str::to_owned));
            }
            for r in &self.result {
                if !matches!(r, Action::Command { .. }) {
                    lore.extend(r.describe(shops).split('\n').map(str::to_owned));
                }
            }
            if self.times_purchasable > 0 {
                lore.push(format!("Times purchasable: {}", self.times_purchasable));
            }
            stack.meta.lore = lore;
        }
        if !stack.meta.lore.is_empty() {
            stack.meta.lore = stack.meta.lore.iter().map(|l| self.placeholders(l, buyer, shops)).collect();
        }
        Some(stack)
    }

    /// `changeCost` / `changeResult`: the first action of this kind is replaced (or removed with `None`), else appended.
    pub fn change_action(list: &mut Vec<Action>, kind: Kind, delta: Option<Action>) {
        if let Some(i) = list.iter().position(|a| a.kind() == kind) {
            match delta {
                None => {
                    list.remove(i);
                }
                Some(d) => list[i] = d,
            }
            return;
        }
        if let Some(d) = delta {
            list.push(d);
        }
    }

    pub fn from_node(n: &Node, cfg: &ShopsConfig) -> ShopItem {
        let mut s = ShopItem::new(cfg);
        // A key that is not there leaves the field null: the loader sets what it reads.
        s.already_purchased_message = text_of(n, "alreadyPurchasedMessage");
        s.click_to_confirm_message = text_of(n, "clickToConfirmMessage");
        s.cost_message = text_of(n, "costMessage");
        s.result_message = text_of(n, "resultMessage");
        s.is_clickable_expression = text_of(n, "isClickableExpression");
        s.is_visible_expression = text_of(n, "isVisibleExpression");
        s.cost = n.get("cost").map(|l| listed(l).into_iter().filter_map(Action::from_node).collect()).unwrap_or_default();
        s.result = n.get("result").map(|l| listed(l).into_iter().filter_map(Action::from_node).collect()).unwrap_or_default();
        s.display = n.get("display").and_then(StackData::from_node);
        if let Some(v) = n.get("globalTimesPurchasable").and_then(Node::as_i64) {
            s.global_times_purchasable = v as i32;
        }
        if let Some(v) = n.get("timesPurchasable").and_then(Node::as_i64) {
            s.times_purchasable = v as i32;
        }
        if let Some(v) = n.get("maxRepeatsOnShiftClick").and_then(Node::as_bool) {
            s.max_repeats_on_shift_click = v;
        }
        if let Some(v) = n.get("npurchases").and_then(Node::as_i64) {
            s.npurchases = v as i32;
        }
        if let Some(p) = n.get("purchases") {
            for e in p.entries() {
                if let (Some(u), Some(c)) = (crate::horizon_citizens_core::ids::parse_uuid(&e.key), e.value.as_i64()) {
                    s.purchases.insert(u, c as i32);
                }
            }
        }
        s
    }

    pub fn to_node(&self) -> Node {
        let mut n = Node::map();
        set_text(&mut n, "alreadyPurchasedMessage", &self.already_purchased_message);
        set_text(&mut n, "clickToConfirmMessage", &self.click_to_confirm_message);
        if !self.cost.is_empty() {
            n.set("cost", indexed(self.cost.iter().map(Action::to_node).collect()));
        }
        set_text(&mut n, "costMessage", &self.cost_message);
        if let Some(d) = &self.display {
            n.set("display", d.to_node());
        }
        n.set("globalTimesPurchasable", Node::int(i64::from(self.global_times_purchasable)));
        set_text(&mut n, "isClickableExpression", &self.is_clickable_expression);
        set_text(&mut n, "isVisibleExpression", &self.is_visible_expression);
        n.set("maxRepeatsOnShiftClick", Node::bool(self.max_repeats_on_shift_click));
        n.set("npurchases", Node::int(i64::from(self.npurchases)));
        if !self.purchases.is_empty() {
            let mut p = Node::map();
            for (u, c) in &self.purchases {
                p.set(&crate::horizon_citizens_core::ids::format_uuid(*u), Node::int(i64::from(*c)));
            }
            n.set("purchases", p);
        }
        if !self.result.is_empty() {
            n.set("result", indexed(self.result.iter().map(Action::to_node).collect()));
        }
        set_text(&mut n, "resultMessage", &self.result_message);
        n.set("timesPurchasable", Node::int(i64::from(self.times_purchasable)));
        n
    }

    /// `NPCShopItem.onClick`. `storage` is the storage the shop shows: only a limited one is touched.
    pub fn click(&mut self, storage: &mut Storage, buyer: &Buyer, shift: bool, second_click: bool, shops: &dyn Lookup) -> Outcome {
        let mut out = Outcome { levels: buyer.levels, inv: buyer.inv.clone(), money: buyer.money, ..Outcome::default() };
        if let Some(expr) = &self.is_clickable_expression
            && !eval_pass(expr, Some(buyer))
        {
            return out;
        }
        if self.global_times_purchasable > 0 && self.npurchases >= self.global_times_purchasable {
            return out;
        }
        if self.times_purchasable > 0 && self.purchases.get(&buyer.uuid).copied().unwrap_or(0) >= self.times_purchasable {
            if let Some(m) = &self.already_purchased_message {
                out.messages.push(self.placeholders(m, Some(buyer), shops));
            }
            return out;
        }
        if let Some(m) = &self.click_to_confirm_message
            && !second_click
        {
            out.messages.push(self.placeholders(m, Some(buyer), shops));
            return out;
        }
        let start = Work {
            levels: buyer.levels,
            money: buyer.money,
            balance: storage.balance,
            inv: buyer.inv.clone(),
            perms: buyer.perms.clone(),
            storage: storage.inventory.iter().cloned().map(Some).collect(),
        };
        let mut work = start.clone();
        let mut max = i64::MAX;
        if self.max_repeats_on_shift_click && shift {
            for a in &self.cost {
                let r = a.max_repeats(&work);
                if r != -1 {
                    max = max.min(r);
                }
            }
            if max == 0 {
                return out;
            }
        }
        let repeats = if max == i64::MAX { 1 } else { max };
        let mut effects: Vec<Effect> = Vec::new();
        if !apply(&self.cost, true, storage, buyer, repeats, &mut work, &mut effects, shops) {
            if let Some(m) = &self.cost_message {
                out.messages.push(self.placeholders(m, Some(buyer), shops));
            }
            out.effects = effects;
            return out;
        }
        if !apply(&self.result, false, storage, buyer, repeats, &mut work, &mut effects, shops) {
            out.effects = effects;
            return out;
        }
        if let Some(m) = &self.result_message {
            out.messages.push(self.placeholders(m, Some(buyer), shops));
        }
        if self.times_purchasable > 0 {
            *self.purchases.entry(buyer.uuid).or_insert(0) += 1;
        }
        if self.global_times_purchasable > 0 {
            self.npurchases += 1;
        }
        if !storage.unlimited {
            storage.inventory = work.storage.into_iter().flatten().filter(|s| s.count > 0).collect();
        }
        // The storage's balance follows every payment, limited or not (`MoneyAction` sets it).
        storage.balance = work.balance;
        out.done = true;
        out.effects = effects;
        out.levels = work.levels;
        out.money = work.money;
        out.inv = work.inv;
        out
    }
}

/// `NPCShopItem.apply`: each action must be possible against the state the earlier ones left; the first
/// that is not undoes the ones before it (their commands and open-shops stay run). `take` picks `take()` or `grant()`.
#[allow(clippy::too_many_arguments)]
fn apply(
    actions: &[Action],
    take: bool,
    storage: &Storage,
    buyer: &Buyer,
    repeats: i64,
    w: &mut Work,
    effects: &mut Vec<Effect>,
    shops: &dyn Lookup,
) -> bool {
    let before = w.clone();
    for a in actions {
        if !possible(a, take, storage, buyer, repeats, w, shops) {
            *w = before;
            return false;
        }
        run(a, take, storage, buyer, repeats, w, effects);
    }
    true
}

fn possible(a: &Action, take: bool, storage: &Storage, buyer: &Buyer, repeats: i64, w: &Work, shops: &dyn Lookup) -> bool {
    match a {
        Action::Item { items, .. } => {
            if take {
                let mut scratch = w.inv.clone();
                (storage.unlimited || storage.can_add(items.len() as i64 * repeats)) && a.take_items(&mut scratch, repeats, false)
            } else {
                let mut st = w.storage.clone();
                (storage.unlimited || a.take_items(&mut st, repeats, false)) && a.can_accept(&w.inv, repeats)
            }
        }
        Action::Permission { permissions } => {
            if take {
                buyer.op || permissions.iter().all(|p| w.perms.contains(&player_placeholders(p, buyer)))
            } else {
                true
            }
        }
        Action::Money { money } => {
            if *money <= 0.0 {
                true
            } else if take {
                w.money >= money * repeats as f64
            } else {
                storage.unlimited || w.balance - money * repeats as f64 >= 0.0
            }
        }
        Action::Command { .. } => true,
        Action::Experience { exp } => !take || w.levels >= exp.saturating_mul(repeats as i32),
        Action::OpenShop { shop_name } => match shop_name.as_deref().and_then(|n| shops.shop_name(n)) {
            Some(n) => shops.can_view(&n, buyer),
            None => false,
        },
        Action::Condition { condition } => match condition {
            None => true,
            Some(c) => eval_bool(c, Some(buyer)),
        },
    }
}

fn run(a: &Action, take: bool, storage: &Storage, buyer: &Buyer, repeats: i64, w: &mut Work, effects: &mut Vec<Effect>) {
    match a {
        Action::Item { .. } => {
            if take {
                if !storage.unlimited {
                    a.give_items(&mut w.storage, repeats);
                }
                a.take_items(&mut w.inv, repeats, true);
            } else {
                if !storage.unlimited {
                    a.take_items(&mut w.storage, repeats, true);
                }
                a.give_items(&mut w.inv, repeats);
            }
        }
        Action::Permission { permissions } => {
            for p in permissions {
                let node = player_placeholders(p, buyer);
                if take {
                    w.perms.remove(&node);
                } else {
                    w.perms.insert(node.clone());
                }
                effects.push(Effect::Perm { node, set: !take });
            }
        }
        Action::Money { money } => {
            if *money > 0.0 {
                let amount = money * repeats as f64;
                if take {
                    w.money -= amount;
                    w.balance += amount;
                } else {
                    w.money += amount;
                    w.balance -= amount;
                }
            }
        }
        Action::Condition { .. } => {}
        Action::Command { commands, server, .. } => {
            for _ in 0..repeats {
                for c in commands {
                    effects.push(Effect::Command { line: run_command_line(c, buyer), as_player: !*server });
                }
            }
        }
        Action::Experience { exp } => {
            let amount = exp.saturating_mul(repeats as i32);
            w.levels = if take { w.levels - amount } else { w.levels + amount };
        }
        Action::OpenShop { shop_name } => {
            if let Some(n) = shop_name {
                effects.push(Effect::OpenShop(n.clone()));
            }
        }
    }
}

/// `Util.runCommand`'s rewriting: `npc`, `waypoints` and `wp` commands get `--id <id>`; the player's
/// placeholders are filled (there is no NPC, so `<id>` stays).
pub fn run_command_line(command: &str, buyer: &Buyer) -> String {
    let mut cmd = command.to_owned();
    if (cmd.starts_with("npc ") || cmd.starts_with("waypoints ") || cmd.starts_with("wp ")) && !cmd.contains("--id ") {
        cmd.push_str(" --id <id>");
    }
    player_placeholders(&cmd, buyer).trim_start_matches('/').to_owned()
}

/// `Placeholders.replace(text, player)`: the player's name and UUID, and the world.
pub fn player_placeholders(s: &str, b: &Buyer) -> String {
    s.replace("<player>", &b.name)
        .replace("<p>", &b.name)
        .replace("%player%", &b.name)
        .replace("<player_uuid>", &crate::horizon_citizens_core::ids::format_uuid(b.uuid))
        .replace("<world>", &b.world)
}

/// `NPCShopPage`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub index: usize,
    pub items: BTreeMap<usize, ShopItem>,
    pub title: Option<String>,
}

/// `NPCShop`.
#[derive(Clone, Debug, PartialEq)]
pub struct Shop {
    pub name: String,
    pub pages: Vec<Page>,
    pub storage: Storage,
    pub title: Option<String>,
    pub shop_type: ShopType,
    pub view_permission: Option<String>,
}

impl Shop {
    pub fn new(name: &str) -> Shop {
        Shop { name: name.to_owned(), pages: Vec::new(), storage: Storage::default(), title: None, shop_type: ShopType::Default, view_permission: None }
    }

    pub fn title(&self) -> String {
        self.title.clone().unwrap_or_default()
    }

    /// `getOrCreatePage`.
    pub fn page_mut(&mut self, page: usize) -> &mut Page {
        while self.pages.len() <= page {
            let index = self.pages.len();
            self.pages.push(Page { index, ..Page::default() });
        }
        &mut self.pages[page]
    }

    /// `removePage(index)`: the page with that index goes, the ones after it move up.
    pub fn remove_page(&mut self, index: usize) {
        if index < self.pages.len() {
            self.pages.remove(index);
            for (i, p) in self.pages.iter_mut().enumerate() {
                p.index = i;
            }
        }
    }

    /// `setPermission`: empty clears it.
    pub fn set_permission(&mut self, p: Option<String>) {
        self.view_permission = p.filter(|s| !s.is_empty());
    }

    /// `canView`: `viewPermission` then the global node, both asked of `holds`.
    pub fn can_view(&self, global: &str, holds: &dyn Fn(&str) -> bool) -> bool {
        if let Some(p) = &self.view_permission
            && !holds(p)
        {
            return false;
        }
        global.is_empty() || holds(global)
    }

    pub fn to_node(&self) -> Node {
        let mut n = Node::map();
        if !self.pages.is_empty() {
            let mut pages = Node::map();
            for (i, p) in self.pages.iter().enumerate() {
                let mut pn = Node::map();
                // `$key` is the index and is not saved; `items` is declared before `title`.
                if !p.items.is_empty() {
                    let mut items = Node::map();
                    for (slot, it) in &p.items {
                        items.set(&slot.to_string(), it.to_node());
                    }
                    pn.set("items", items);
                }
                set_text(&mut pn, "title", &p.title);
                pages.set(&i.to_string(), pn);
            }
            n.set("pages", pages);
        }
        n.set("storage", self.storage.to_node());
        set_text(&mut n, "title", &self.title);
        n.set("type", Node::str(self.shop_type.name()));
        set_text(&mut n, "viewPermission", &self.view_permission);
        n
    }

    pub fn from_node(name: &str, n: &Node, cfg: &ShopsConfig) -> Shop {
        let mut s = Shop::new(name);
        if let Some(pages) = n.get("pages") {
            for (i, pn) in listed(pages).into_iter().enumerate() {
                let mut page = Page { index: i, ..Page::default() };
                page.title = text_of(pn, "title");
                if let Some(items) = pn.get("items") {
                    for e in items.entries() {
                        if let Ok(slot) = e.key.trim().parse::<usize>() {
                            page.items.insert(slot, ShopItem::from_node(&e.value, cfg));
                        }
                    }
                }
                s.pages.push(page);
            }
        }
        if let Some(st) = n.get("storage") {
            s.storage = Storage::from_node(st);
        }
        s.title = text_of(n, "title");
        s.shop_type = text_of(n, "type").and_then(|t| ShopType::parse(&t)).unwrap_or(ShopType::Default);
        s.view_permission = text_of(n, "viewPermission");
        s
    }
}

/// `StoredShops`: `global` and `npc` (keyed by the NPC's UUID).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoredShops {
    pub global: BTreeMap<String, Shop>,
    pub npc: BTreeMap<String, Shop>,
    /// The file as read (header, unknown keys).
    doc: Option<Document>,
}

impl StoredShops {
    pub fn empty() -> StoredShops {
        StoredShops::default()
    }

    pub fn parse(text: &str, cfg: &ShopsConfig) -> Result<StoredShops, yaml::Error> {
        let doc = yaml::parse_document(text)?;
        let mut s = StoredShops::default();
        if let Some(g) = doc.root.get("global") {
            for e in g.entries() {
                s.global.insert(e.key.clone(), Shop::from_node(&e.key, &e.value, cfg));
            }
        }
        if let Some(g) = doc.root.get("npc") {
            for e in g.entries() {
                s.npc.insert(e.key.clone(), Shop::from_node(&e.key, &e.value, cfg));
            }
        }
        s.doc = Some(doc);
        Ok(s)
    }

    pub fn load(path: &Path, cfg: &ShopsConfig) -> Result<StoredShops, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => StoredShops::parse(&text, cfg).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StoredShops::empty()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn to_yaml(&self) -> String {
        let mut doc =
            self.doc.clone().unwrap_or_else(|| Document { leading: vec![format!("# {HEADER}")], root: Node::map(), trailing: Vec::new() });
        let mut g = Node::map();
        for (k, s) in &self.global {
            g.set(k, s.to_node());
        }
        let mut n = Node::map();
        for (k, s) in &self.npc {
            n.set(k, s.to_node());
        }
        doc.root.set("global", g);
        doc.root.set("npc", n);
        yaml::write_document(&doc)
    }

    /// Written whole, through a temporary file and a rename.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = path.with_extension("yml.tmp");
        std::fs::write(&tmp, self.to_yaml()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// `addNamedShop`.
    pub fn add_named(&mut self, name: &str) -> &mut Shop {
        self.global.entry(name.to_owned()).or_insert_with(|| Shop::new(name))
    }

    /// `deleteShop`: the NPC shop of that name, else the global one.
    pub fn delete(&mut self, name: &str) {
        if self.npc.remove(name).is_none() {
            self.global.remove(name);
        }
    }

    /// `getShop`: `npc_uuid_of` turns a number (an NPC id) into that NPC's UUID text. NPC shops come first;
    /// the flag says it is one.
    pub fn find(&self, name: &str, npc_uuid_of: &dyn Fn(i32) -> Option<String>) -> Option<(&Shop, bool)> {
        let key = name.parse::<i32>().ok().and_then(npc_uuid_of).unwrap_or_else(|| name.to_owned());
        if let Some(s) = self.npc.get(&key) {
            return Some((s, true));
        }
        self.global.get(&key).map(|s| (s, false))
    }
}

// ---- expressions (Molang, through `expr`) ------------------------------------------------------

pub use crate::horizon_citizens_core::expr::{default_markup, is_expression};
use crate::horizon_citizens_core::expr::{self, Value};

/// The scope a shop expression sees: the buyer's `<player>` style text for `papi`, and PlaceholderAPI behind it.
fn buyer_scope(buyer: Option<&Buyer>) -> expr::Scope {
    let mut sc = expr::Scope::new();
    if let Some(b) = buyer {
        sc.player = Some(b.uuid);
        sc.subst = vec![
            ("<player>".to_owned(), b.name.clone()),
            ("<p>".to_owned(), b.name.clone()),
            ("%player%".to_owned(), b.name.clone()),
            ("<player_uuid>".to_owned(), crate::horizon_citizens_core::ids::format_uuid(b.uuid)),
            ("<world>".to_owned(), b.world.clone()),
        ];
        sc.set("player.name", Value::Str(b.name.clone()));
        sc.set("player.world", Value::Str(b.world.clone()));
        sc.set("player.level", Value::Num(f64::from(b.levels)));
    }
    sc
}

/// `evaluateAsBoolean` of the compiled expression; `None` when it does not compile (no backticks, a syntax error).
/// Any prefix compiles as Molang, the one engine Citizens registers.
pub fn eval(expr_text: &str, buyer: Option<&Buyer>) -> Option<bool> {
    let c = expr::compile_markup(expr_text).ok()?;
    let mut sc = buyer_scope(buyer);
    Some(c.eval_bool(&mut sc))
}

/// `ConditionAction`: an expression that does not compile fails (the original throws on the missing compiled form).
pub fn eval_bool(expr_text: &str, buyer: Option<&Buyer>) -> bool {
    eval(expr_text, buyer).unwrap_or(false)
}

/// The visible and clickable expressions: one that does not compile is skipped (the original prints the error and goes on).
pub fn eval_pass(expr_text: &str, buyer: Option<&Buyer>) -> bool {
    eval(expr_text, buyer).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoShops;
    impl Lookup for NoShops {
        fn shop_name(&self, name: &str) -> Option<String> {
            (name == "bank").then(|| "bank".to_owned())
        }
        fn can_view(&self, _: &str, _: &Buyer) -> bool {
            true
        }
    }

    fn apples(n: i64) -> StackData {
        StackData::new("minecraft:apple", n)
    }

    fn buyer(inv: Vec<(usize, StackData)>, levels: i32) -> Buyer {
        let mut slots = vec![None; 36];
        for (i, s) in inv {
            slots[i] = Some(s);
        }
        Buyer { uuid: 7, name: "Alex".into(), world: "world".into(), levels, inv: slots, perms: BTreeSet::new(), op: false, money: 0.0 }
    }

    fn item(cost: Vec<Action>, result: Vec<Action>) -> ShopItem {
        let mut it = ShopItem::new(&ShopsConfig::default());
        it.display = Some(StackData::new("minecraft:diamond", 1));
        it.cost = cost;
        it.result = result;
        it
    }

    fn pay_apples(n: i64) -> Action {
        Action::Item { items: vec![apples(n)], meta_filter: vec![], require_undamaged: true }
    }

    #[test]
    fn shop_type_cells_are_the_originals() {
        assert_eq!((ShopType::Default.size(), ShopType::Default.prev_slot(), ShopType::Default.edit_slot(), ShopType::Default.next_slot()), (45, 39, 40, 41));
        assert_eq!((ShopType::Chest4x9.prev_slot(), ShopType::Chest4x9.edit_slot(), ShopType::Chest4x9.next_slot()), (30, 31, 32));
        assert_eq!((ShopType::Chest1x9.size(), ShopType::Chest1x9.prev_slot(), ShopType::Chest1x9.edit_slot(), ShopType::Chest1x9.next_slot()), (9, 7, 6, 8));
        assert_eq!(ShopType::Trader.size(), 45);
        assert_eq!(ShopType::parse("chest_2x9"), Some(ShopType::Chest2x9));
    }

    #[test]
    fn buying_takes_the_cost_and_gives_the_result() {
        let mut it = item(vec![pay_apples(3)], vec![Action::Experience { exp: 2 }]);
        let mut st = Storage::default();
        let b = buyer(vec![(0, apples(2)), (5, apples(4))], 1);
        let o = it.click(&mut st, &b, false, false, &NoShops);
        assert!(o.done);
        assert_eq!(o.inv[0], None, "the first stack pays 2");
        assert_eq!(o.inv[5].as_ref().map(|s| s.count), Some(3), "the second pays 1 more");
        assert_eq!(o.levels, 3);
    }

    #[test]
    fn missing_cost_sends_the_cost_message_and_changes_nothing() {
        let mut it = item(vec![pay_apples(3)], vec![]);
        it.cost_message = Some("Too poor".into());
        let mut st = Storage::default();
        let b = buyer(vec![(0, apples(2))], 0);
        let o = it.click(&mut st, &b, false, false, &NoShops);
        assert!(!o.done);
        assert_eq!(o.messages, vec!["Too poor".to_owned()]);
        assert_eq!(o.inv[0].as_ref().map(|s| s.count), Some(2));
    }

    #[test]
    fn experience_cost_needs_the_levels() {
        let mut it = item(vec![Action::Experience { exp: 5 }], vec![]);
        let mut st = Storage::default();
        assert!(!it.click(&mut st, &buyer(vec![], 4), false, false, &NoShops).done);
        let o = it.click(&mut st, &buyer(vec![], 5), false, false, &NoShops);
        assert!(o.done);
        assert_eq!(o.levels, 0);
    }

    #[test]
    fn times_purchasable_counts_per_player_and_says_already_purchased() {
        let mut it = item(vec![], vec![]);
        it.times_purchasable = 1;
        it.already_purchased_message = Some("Sold out for you".into());
        let mut st = Storage::default();
        let b = buyer(vec![], 0);
        assert!(it.click(&mut st, &b, false, false, &NoShops).done);
        assert_eq!(it.purchases.get(&7), Some(&1));
        let o = it.click(&mut st, &b, false, false, &NoShops);
        assert!(!o.done);
        assert_eq!(o.messages, vec!["Sold out for you".to_owned()]);
    }

    #[test]
    fn global_limit_stops_silently() {
        let mut it = item(vec![], vec![]);
        it.global_times_purchasable = 2;
        let mut st = Storage::default();
        let b = buyer(vec![], 0);
        assert!(it.click(&mut st, &b, false, false, &NoShops).done);
        assert!(it.click(&mut st, &b, false, false, &NoShops).done);
        let o = it.click(&mut st, &b, false, false, &NoShops);
        assert!(!o.done && o.messages.is_empty());
        assert_eq!(it.npurchases, 2);
    }

    #[test]
    fn click_to_confirm_needs_a_second_click() {
        let mut it = item(vec![Action::Experience { exp: 1 }], vec![]);
        it.click_to_confirm_message = Some("Click again to buy <cost>".into());
        let mut st = Storage::default();
        let b = buyer(vec![], 3);
        let o = it.click(&mut st, &b, false, false, &NoShops);
        assert!(!o.done);
        assert_eq!(o.messages, vec!["Click again to buy 1 level".to_owned()]);
        assert!(it.click(&mut st, &b, false, true, &NoShops).done);
    }

    #[test]
    fn shift_click_buys_as_many_as_the_cost_allows() {
        let mut it = item(vec![Action::Experience { exp: 2 }], vec![Action::Experience { exp: 1 }]);
        it.max_repeats_on_shift_click = true;
        let mut st = Storage::default();
        let o = it.click(&mut st, &buyer(vec![], 7), true, false, &NoShops);
        assert!(o.done);
        assert_eq!(o.levels, 7 - 6 + 3, "three repeats: pay 6, get 3");
    }

    #[test]
    fn result_that_does_not_fit_undoes_the_cost() {
        let mut it = item(vec![Action::Experience { exp: 1 }], vec![pay_apples(1)]);
        let mut st = Storage::default();
        let mut inv = Vec::new();
        for i in 0..36 {
            inv.push((i, StackData::new("minecraft:stone", 64)));
        }
        let o = it.click(&mut st, &buyer(inv, 4), false, false, &NoShops);
        assert!(!o.done);
        assert_eq!(o.levels, 4);
    }

    #[test]
    fn give_items_merges_into_a_similar_stack() {
        let a = pay_apples(5);
        let mut inv = vec![Some(apples(62)), None];
        a.give_items(&mut inv, 1);
        assert_eq!(inv[0].as_ref().map(|s| s.count), Some(64));
        assert_eq!(inv[1].as_ref().map(|s| s.count), Some(3));
    }

    #[test]
    fn damaged_items_do_not_pay_when_undamaged_is_required() {
        let mut worn = StackData::new("minecraft:diamond_sword", 1);
        worn.meta.damage = Some(3);
        let a = Action::Item { items: vec![StackData::new("minecraft:diamond_sword", 1)], meta_filter: vec![], require_undamaged: true };
        let mut inv = vec![Some(worn.clone())];
        assert!(!a.take_items(&mut inv, 1, false));
        let b = Action::Item { items: vec![worn.clone()], meta_filter: vec![], require_undamaged: false };
        assert!(b.take_items(&mut inv, 1, true));
        assert_eq!(inv[0], None);
    }

    #[test]
    fn command_action_runs_per_repeat_with_the_player_filled_in() {
        let mut it = item(vec![], vec![Action::Command { commands: vec!["give <player> stone".into(), "say hi".into()], op: false, server: true }]);
        let mut st = Storage::default();
        let o = it.click(&mut st, &buyer(vec![], 0), false, false, &NoShops);
        assert_eq!(
            o.effects,
            vec![
                Effect::Command { line: "give Alex stone".into(), as_player: false },
                Effect::Command { line: "say hi".into(), as_player: false },
            ]
        );
    }

    #[test]
    fn npc_commands_get_the_id_flag_as_in_util_run_command() {
        let b = buyer(vec![], 0);
        assert_eq!(run_command_line("npc create X", &b), "npc create X --id <id>");
        assert_eq!(run_command_line("wp add --id 4", &b), "wp add --id 4");
        assert_eq!(run_command_line("/tp <p> 0 0 0", &b), "tp Alex 0 0 0");
    }

    #[test]
    fn permission_action_takes_what_the_player_holds() {
        let mut it = item(vec![Action::Permission { permissions: vec!["vip.use".into()] }], vec![]);
        let mut st = Storage::default();
        assert!(!it.click(&mut st, &buyer(vec![], 0), false, false, &NoShops).done);
        let mut b = buyer(vec![], 0);
        b.perms.insert("vip.use".into());
        let o = it.click(&mut st, &b, false, false, &NoShops);
        assert!(o.done);
        assert_eq!(o.effects, vec![Effect::Perm { node: "vip.use".into(), set: false }]);
        let mut gift = item(vec![], vec![Action::Permission { permissions: vec!["kit.<player>".into()] }]);
        let o = gift.click(&mut st, &buyer(vec![], 0), false, false, &NoShops);
        assert_eq!(o.effects, vec![Effect::Perm { node: "kit.Alex".into(), set: true }]);
    }

    #[test]
    fn open_shop_needs_an_existing_viewable_shop() {
        let mut it = item(vec![], vec![Action::OpenShop { shop_name: Some("bank".into()) }]);
        let mut st = Storage::default();
        let o = it.click(&mut st, &buyer(vec![], 0), false, false, &NoShops);
        assert_eq!(o.effects, vec![Effect::OpenShop("bank".into())]);
        let mut gone = item(vec![], vec![Action::OpenShop { shop_name: Some("nope".into()) }]);
        assert!(!gone.click(&mut st, &buyer(vec![], 0), false, false, &NoShops).done);
        assert_eq!(Action::OpenShop { shop_name: None }.describe(&NoShops), "Open shop (currently unset)");
        assert_eq!(Action::OpenShop { shop_name: Some("bank".into()) }.describe(&NoShops), "Open bank");
    }

    #[test]
    fn money_is_taken_from_the_buyer_and_kept_in_the_storage() {
        let mut it = item(vec![Action::Money { money: 5.0 }], vec![Action::Experience { exp: 1 }]);
        let mut st = Storage::default();
        let mut rich = buyer(vec![], 0);
        rich.money = 12.0;
        let o = it.click(&mut st, &rich, false, false, &NoShops);
        assert!(o.done);
        assert_eq!(o.money, 7.0);
        assert_eq!(st.balance, 5.0);
        let mut poor = buyer(vec![], 0);
        poor.money = 4.99;
        assert!(!it.click(&mut st, &poor, false, false, &NoShops).done);
    }

    #[test]
    fn shift_click_buys_as_often_as_the_balance_allows() {
        let mut it = item(vec![Action::Money { money: 5.0 }], vec![Action::Experience { exp: 1 }]);
        it.max_repeats_on_shift_click = true;
        let mut st = Storage::default();
        let mut rich = buyer(vec![], 0);
        rich.money = 17.0;
        let o = it.click(&mut st, &rich, true, false, &NoShops);
        assert!(o.done);
        assert_eq!((o.money, o.levels), (2.0, 3));
    }

    #[test]
    fn a_money_result_pays_out_of_a_limited_balance() {
        let mut it = item(vec![Action::Experience { exp: 1 }], vec![Action::Money { money: 5.0 }]);
        let mut st = Storage { unlimited: false, balance: 7.0, ..Storage::default() };
        let mut b = buyer(vec![], 3);
        b.money = 1.0;
        let o = it.click(&mut st, &b, false, false, &NoShops);
        assert!(o.done);
        assert_eq!((o.money, st.balance), (6.0, 2.0));
        // Not enough in the shop's balance for another.
        let again = it.click(&mut st, &b, false, false, &NoShops);
        assert!(!again.done);
    }

    #[test]
    fn money_cannot_be_paid_without_an_economy() {
        let mut it = item(vec![Action::Money { money: 5.0 }], vec![]);
        let mut st = Storage::default();
        assert!(!it.click(&mut st, &buyer(vec![], 0), false, false, &NoShops).done);
        let mut free = item(vec![Action::Money { money: 0.0 }], vec![]);
        assert!(free.click(&mut st, &buyer(vec![], 0), false, false, &NoShops).done);
    }

    #[test]
    fn limited_storage_pays_out_and_takes_in() {
        let mut st = Storage { unlimited: false, inventory: vec![apples(10)], ..Storage::default() };
        let mut it = item(vec![], vec![pay_apples(4)]);
        let o = it.click(&mut st, &buyer(vec![], 0), false, false, &NoShops);
        assert!(o.done);
        assert_eq!(st.inventory.iter().map(|s| s.count).collect::<Vec<_>>(), vec![6]);
        assert_eq!(o.inv[0].as_ref().map(|s| s.count), Some(4));
        let mut empty = Storage { unlimited: false, ..Storage::default() };
        assert!(!it.click(&mut empty, &buyer(vec![], 0), false, false, &NoShops).done);
    }

    #[test]
    fn describe_matches_the_originals_text() {
        let l = NoShops;
        assert_eq!(Action::Experience { exp: 1 }.describe(&l), "1 level");
        assert_eq!(Action::Experience { exp: 30 }.describe(&l), "30 levels");
        assert_eq!(pay_apples(3).describe(&l), "3 Apple");
        assert_eq!(Action::Command { commands: vec!["a".into(), "b".into()], op: false, server: false }.describe(&l), "2 command\na\nb");
        let five: Vec<String> = ["1", "2", "3", "4", "5"].iter().map(|s| s.to_string()).collect();
        assert_eq!(Action::Permission { permissions: five }.describe(&l), "5 permissions\n1\n2\n3\n4...");
        let two = Action::Item { items: vec![apples(1), StackData::new("minecraft:diamond_sword", 2)], meta_filter: vec![], require_undamaged: true };
        assert_eq!(two.describe(&l), "2 items\n1 Apple\n2 Diamond Sword");
        assert_eq!(Action::Money { money: 5.0 }.describe(&l), "5.0");
    }

    #[test]
    fn placeholders_fill_cost_result_and_times() {
        let mut it = item(vec![Action::Experience { exp: 2 }], vec![pay_apples(1)]);
        it.times_purchasable = 3;
        let l = NoShops;
        assert_eq!(it.placeholders("<cost> -> <result> (<times_purchasable>) <player>", Some(&buyer(vec![], 0)), &l), "2 levels -> 1 Apple (3) Alex");
        let cfg = ShopsConfig { add_default_item_description: true, ..ShopsConfig::default() };
        let shown = it.display_item(None, &cfg, &l).unwrap();
        assert_eq!(shown.meta.lore, vec!["2 levels", "1 Apple", "Times purchasable: 3"]);
    }

    #[test]
    fn default_item_settings_seed_a_new_item() {
        let cfg = ShopsConfig {
            default_item: DefaultItem { cost_message: "No".into(), times_purchasable: 2, name: "<itemname>!".into(), ..DefaultItem::default() },
            ..ShopsConfig::default()
        };
        let mut it = ShopItem::new(&cfg);
        assert_eq!((it.cost_message.as_deref(), it.times_purchasable), (Some("No"), 2));
        it.set_display_item(Some(StackData::new("minecraft:diamond_sword", 1)));
        assert_eq!(it.display.unwrap().meta.display_name.as_deref(), Some("Diamond Sword!"));
    }

    #[test]
    fn expressions_compare_and_branch() {
        let b = buyer(vec![], 0);
        assert!(eval_bool("`1 + 1 == 2`", Some(&b)));
        assert!(!eval_bool("`3 < 2 || 0`", Some(&b)));
        assert!(eval_bool("`papi('<player>') == 'Alex'`", Some(&b)));
        assert!(!eval_bool("`papi('<player>') == 'Bob'`", Some(&b)));
        assert!(eval_bool("`(2 > 1) ? 1 : 0`", Some(&b)));
        assert!(eval_bool("js`true`", Some(&b)), "an unknown prefix compiles as Molang, as in Citizens");
        assert!(eval_bool("`v.a = 3; v.a * 2 == 6`", Some(&b)));
        assert!(eval_bool("`player.level >= 0`", Some(&b)));
        assert!(!eval_bool("1 == 1", Some(&b)), "not wrapped in backticks");
        assert!(eval_pass("1 == 1", Some(&b)), "a visible expression that does not compile is skipped");
        assert!(!eval_pass("`0`", Some(&b)));
        assert_eq!(default_markup("1 == 1"), "`1 == 1`");
        assert_eq!(default_markup("molang`1`"), "molang`1`");
    }

    const FILE: &str = "# Citizens NPC Shops
global:
  bank:
    pages:
      '0':
        items:
          '4':
            cost:
              '0':
                exp: 5
                type: experience
            display:
              type: diamond
              amount: 1
            globalTimesPurchasable: 0
            maxRepeatsOnShiftClick: false
            npurchases: 0
            result:
              '0':
                commands:
                  '0': give <player> stone
                op: false
                server: true
                type: command
            timesPurchasable: 2
        title: Page one
    storage:
      balance: 0.0
      inventorySizeLimit: -1
      unlimited: true
    title: Bank
    type: CHEST_4X9
    viewPermission: bank.view
npc: {}
";

    #[test]
    fn shops_yml_loads_and_saves_in_the_same_shape() {
        let cfg = ShopsConfig::default();
        let s = StoredShops::parse(FILE, &cfg).unwrap();
        let bank = &s.global["bank"];
        assert_eq!(bank.shop_type, ShopType::Chest4x9);
        assert_eq!(bank.title(), "Bank");
        assert_eq!(bank.view_permission.as_deref(), Some("bank.view"));
        assert_eq!(bank.pages[0].title.as_deref(), Some("Page one"));
        let it = &bank.pages[0].items[&4];
        assert_eq!(it.times_purchasable, 2);
        assert_eq!(it.cost, vec![Action::Experience { exp: 5 }]);
        assert_eq!(it.result, vec![Action::Command { commands: vec!["give <player> stone".into()], op: false, server: true }]);
        let text = s.to_yaml();
        let again = StoredShops::parse(&text, &cfg).unwrap();
        assert_eq!(again.global, s.global);
        assert!(text.starts_with("# Citizens NPC Shops\n"), "{text}");
        assert!(text.contains("type: CHEST_4X9"), "{text}");
        assert!(text.contains("type: experience"), "{text}");
    }

    #[test]
    fn stored_shops_find_by_npc_id_and_delete_the_npc_shop_first() {
        let mut s = StoredShops::empty();
        s.add_named("a");
        s.npc.insert("uuid-1".into(), Shop::new("uuid-1"));
        let resolve = |id: i32| (id == 3).then(|| "uuid-1".to_owned());
        assert_eq!(s.find("3", &resolve).map(|(sh, npc)| (sh.name.clone(), npc)), Some(("uuid-1".to_owned(), true)));
        assert_eq!(s.find("a", &resolve).map(|(_, npc)| npc), Some(false));
        assert!(s.find("zzz", &resolve).is_none());
        s.delete("uuid-1");
        assert!(s.npc.is_empty() && s.global.len() == 1);
        s.delete("a");
        assert!(s.global.is_empty());
    }

    #[test]
    fn pages_remove_and_renumber() {
        let mut sh = Shop::new("x");
        sh.page_mut(2);
        assert_eq!(sh.pages.len(), 3);
        sh.pages[1].title = Some("mid".into());
        sh.remove_page(0);
        assert_eq!(sh.pages.iter().map(|p| p.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(sh.pages[0].title.as_deref(), Some("mid"));
    }

    #[test]
    fn change_action_replaces_removes_or_appends() {
        let mut list = vec![Action::Experience { exp: 1 }];
        ShopItem::change_action(&mut list, Kind::Experience, Some(Action::Experience { exp: 9 }));
        assert_eq!(list, vec![Action::Experience { exp: 9 }]);
        ShopItem::change_action(&mut list, Kind::Command, Some(Action::new(Kind::Command)));
        assert_eq!(list.len(), 2);
        ShopItem::change_action(&mut list, Kind::Experience, None);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn view_permission_and_global_node() {
        let mut sh = Shop::new("x");
        assert!(sh.can_view("", &|_| false));
        assert!(!sh.can_view("shops.use", &|_| false));
        sh.set_permission(Some("vip".into()));
        assert!(!sh.can_view("", &|p| p != "vip"));
        sh.set_permission(Some(String::new()));
        assert_eq!(sh.view_permission, None);
    }
}
