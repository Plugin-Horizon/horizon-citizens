//! The shop menus (`ShopTrait`): the paged viewer, the settings editor, the contents editor, the page
//! and item editors, the action editors and the storage viewer, as chest menus; text input goes through
//! the player's next chat line (what `InputMenus.runChatStringSetter` does; the anvil of `stringSetter`
//! is not available as a menu).
//!
//! A click that the original handles with the cursor (placing an item) is a click on one of the player's
//! own stacks here ([`MenuAction::Read`]); moving a stack out of or into the storage is [`MenuAction::Take`]
//! and [`MenuAction::Deposit`], as for `/npc inventory`.

use super::{MenuAction, Op, Sender, Trade, World};
use crate::horizon_citizens_core::commands;
use crate::horizon_citizens_core::npc::StackData;
use crate::horizon_citizens_core::shop::{self, Action, Buyer, Effect, Kind, Lookup, Outcome, Shop, ShopItem, ShopType, Storage};
use crate::horizon_citizens_core::tree::TreeExt;
use std::collections::{BTreeSet, HashMap};

/// A stored shop: an NPC's own (keyed by the NPC's UUID) or a named global one.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Npc(String),
    Global(String),
}

/// Where a storage lives: in a shop, or in an NPC's `shop` trait.
#[derive(Clone, Debug, PartialEq)]
pub enum Src {
    Shop(Target),
    Trait(i32),
}

/// What the host read about a player before a shop interaction.
#[derive(Clone, Debug, Default)]
pub struct ShopPlayer {
    pub name: String,
    pub world: String,
    pub levels: i32,
    /// The 36 main-inventory slots.
    pub inv: Vec<Option<StackData>>,
    /// The wanted nodes ([`World::shop_wanted_nodes`]) the player holds.
    pub perms: BTreeSet<String>,
    pub op: bool,
}

const NO_PERMISSION: &str = "You don't have permission to execute that command.";
/// A chat prompt that nobody answered in five minutes is dropped.
const PROMPT_TICKS: u64 = 6000;

#[derive(Clone, Debug)]
enum Screen {
    Viewer { shop: Target, src: Src, page: usize, last: Option<(usize, usize)> },
    Settings { shop: Target },
    Contents { shop: Target, page: usize, copying: Option<ShopItem> },
    PageSettings { shop: Target, page: usize, remove: bool },
    ItemEditor { shop: Target, page: usize, slot: usize, base: Option<ShopItem>, modified: ShopItem },
    List { cost: bool, work: Action, op_toggle: bool },
    Items { cost: bool, work: Action, slots: Vec<Option<StackData>> },
    Picker { shop: Target },
    Storage { src: Src, slots: Vec<Option<StackData>> },
}

#[derive(Clone, Debug, PartialEq)]
enum Act {
    None,
    Prev,
    Next,
    ViewerItem(usize),
    SPerm,
    SItems,
    STitle,
    SRight,
    SType,
    SStorage,
    CSlot(usize),
    CPrev,
    CNext,
    CEdit,
    PTitle,
    PRemove,
    IDisplay,
    IReset,
    ITimes,
    IGlobal,
    IResult,
    IConfirm,
    IShift,
    IVisible,
    IClickable,
    IName,
    IDesc,
    ICancel,
    IRemove,
    ISave,
    IAction(bool, Kind),
    LSlot(usize),
    LServer,
    LOp,
    ItemsSlot(usize),
    IUndamaged,
    IFilter,
    Pick(ShopType),
    StUnlimited,
    StLimit,
    StSlot(usize),
}

#[derive(Clone, Debug, PartialEq)]
enum PromptFor {
    ShopPerm,
    ShopTitle,
    PageTitle,
    ItemTimes,
    ItemGlobal,
    ItemResult,
    ItemConfirm,
    ItemVisible,
    ItemClickable,
    ItemName,
    ItemDescription,
    ListValue(usize),
    ItemsFilter,
    StorageLimit,
    ActExp(bool),
    ActOpenShop(bool),
    ActCondition(bool),
}

pub(super) struct Session {
    player: u128,
    sender: Option<Sender>,
    /// The NPC whose trait the editor edits.
    npc: Option<i32>,
    window: Option<i32>,
    title: String,
    rows: u8,
    acts: Vec<Act>,
    stack: Vec<Screen>,
    prompting: bool,
    dead: bool,
}

#[derive(Default)]
pub(super) struct ShopUi {
    sessions: HashMap<u128, Session>,
    prompts: HashMap<u128, (PromptFor, u64)>,
    /// (due tick, player, shop name): an open-shop action shows its shop a tick later.
    pending: Vec<(u64, u128, String)>,
}

impl ShopUi {
    /// Another kind of menu replaces the shop's: its session and prompt go.
    pub(super) fn drop_session(&mut self, player: u128) {
        self.sessions.remove(&player);
        self.prompts.remove(&player);
    }
}

struct Rendered {
    title: String,
    rows: u8,
    items: Vec<Option<StackData>>,
    acts: Vec<Act>,
}

impl Rendered {
    fn new(title: &str, rows: u8) -> Rendered {
        let n = usize::from(rows) * 9;
        Rendered { title: title.to_owned(), rows, items: vec![None; n], acts: vec![Act::None; n] }
    }

    fn put(&mut self, slot: usize, item: StackData, act: Act) {
        if slot < self.items.len() {
            self.items[slot] = Some(item);
            self.acts[slot] = act;
        }
    }

    /// A slot with a click handler and no item.
    fn act(&mut self, slot: usize, act: Act) {
        if slot < self.acts.len() {
            self.acts[slot] = act;
        }
    }
}

fn lines(s: &str) -> Vec<String> {
    s.replace("<br>", "\n").split('\n').map(str::to_owned).collect()
}

fn icon(id: &str, name: &str, lore: &[String]) -> StackData {
    let mut s = StackData::new(id, 1);
    s.meta.display_name = Some(name.trim_end_matches('\n').to_owned());
    s.meta.lore = lore.to_vec();
    s
}

fn on_off(on: bool) -> String {
    if on { "&aOn".to_owned() } else { "&cOff".to_owned() }
}

fn unset(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "Unset".to_owned())
}

fn java_null(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "null".to_owned())
}

/// What `InputMenus.stringSetter` hands back for an empty text.
fn prompt_value(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t == "''" || t == "\"\"" || t.eq_ignore_ascii_case("null") || t.eq_ignore_ascii_case("not set") {
        None
    } else {
        Some(text.to_owned())
    }
}

/// The shops the actions and the display check, resolved once so a purchase can borrow its item.
pub(super) struct Snap {
    shops: Vec<SnapShop>,
    ids: Vec<(i32, String)>,
    global: String,
}

struct SnapShop {
    key: String,
    name: String,
    view: Option<String>,
}

impl Snap {
    fn find(&self, q: &str) -> Option<&SnapShop> {
        let key = q
            .parse::<i32>()
            .ok()
            .and_then(|id| self.ids.iter().find(|(i, _)| *i == id).map(|(_, u)| u.clone()))
            .unwrap_or_else(|| q.to_owned());
        self.shops.iter().find(|s| s.key == key).or_else(|| self.shops.iter().find(|s| s.name == key))
    }

    fn allows(&self, shop: &SnapShop, b: &Buyer) -> bool {
        let holds = |node: &str| b.op || b.perms.contains(node);
        if let Some(p) = &shop.view
            && !holds(p)
        {
            return false;
        }
        self.global.is_empty() || holds(&self.global)
    }
}

impl Lookup for Snap {
    fn shop_name(&self, name: &str) -> Option<String> {
        self.find(name).map(|s| s.name.clone())
    }

    fn can_view(&self, name: &str, buyer: &Buyer) -> bool {
        self.find(name).is_some_and(|s| self.allows(s, buyer))
    }
}

impl World {
    // ---- the table -------------------------------------------------------------------

    pub(super) fn shop_snap(&self) -> Snap {
        let mut shops = Vec::new();
        for (k, s) in &self.shops.npc {
            shops.push(SnapShop { key: k.clone(), name: s.name.clone(), view: s.view_permission.clone() });
        }
        for (k, s) in &self.shops.global {
            shops.push(SnapShop { key: k.clone(), name: s.name.clone(), view: s.view_permission.clone() });
        }
        let ids = self
            .saves
            .npcs
            .iter()
            .filter_map(|n| n.uuid().map(|u| (n.id, crate::horizon_citizens_core::ids::format_uuid(u))))
            .collect();
        Snap { shops, ids, global: self.cfg.shops.global_view_permission.clone() }
    }

    fn npc_uuid_text(&self, id: i32) -> Option<String> {
        self.saves.get(id).and_then(|n| n.uuid()).map(crate::horizon_citizens_core::ids::format_uuid)
    }

    /// `StoredShops.getShop(name)`: a number is an NPC id; NPC shops are looked at first.
    pub fn shop_find(&self, name: &str) -> Option<Target> {
        let resolve = |id: i32| self.npc_uuid_text(id);
        let key = name.parse::<i32>().ok().and_then(resolve).unwrap_or_else(|| name.to_owned());
        if self.shops.npc.contains_key(&key) {
            return Some(Target::Npc(key));
        }
        self.shops.global.contains_key(&key).then_some(Target::Global(key))
    }

    pub fn shop_ref(&self, t: &Target) -> Option<&Shop> {
        match t {
            Target::Npc(k) => self.shops.npc.get(k),
            Target::Global(k) => self.shops.global.get(k),
        }
    }

    pub fn shop_mut(&mut self, t: &Target) -> Option<&mut Shop> {
        match t {
            Target::Npc(k) => self.shops.npc.get_mut(k),
            Target::Global(k) => self.shops.global.get_mut(k),
        }
    }

    /// Something in the shops changed: they are written with the next save.
    pub fn shops_changed(&mut self) {
        self.shops_dirty = true;
        self.mark_dirty();
    }

    /// `getOrAddTrait(ShopTrait)`: the trait exists on the NPC with its storage.
    pub fn shop_ensure_trait(&mut self, npc: i32) {
        let Some(n) = self.saves.get_mut(npc) else { return };
        let had = n.has_trait("shop");
        n.add_trait_name("shop");
        if n.map.at(&["traits", "shop", "storage"]).is_none() {
            n.map.put(&["traits", "shop", "storage"], Storage::default().to_node());
        }
        if !had {
            self.mark_dirty();
        }
    }

    /// `ShopTrait.getDefaultShop`: the NPC's own shop, created when it has none.
    pub fn shop_default(&mut self, npc: i32) -> Option<Target> {
        let uuid = self.npc_uuid_text(npc)?;
        self.shop_ensure_trait(npc);
        if !self.shops.npc.contains_key(&uuid) {
            self.shops.npc.insert(uuid.clone(), Shop::new(&uuid));
            self.shops_changed();
        }
        Some(Target::Npc(uuid))
    }

    /// `ShopTrait.setDefaultShop` (`/npc shop copyfrom`).
    pub fn shop_set_default(&mut self, npc: i32, shop: Shop) {
        if let Some(uuid) = self.npc_uuid_text(npc) {
            self.shop_ensure_trait(npc);
            self.shops.npc.insert(uuid, shop);
            self.shops_changed();
        }
    }

    /// The NPC's `rightClickShop`.
    pub fn shop_right_click_name(&self, npc: i32) -> Option<String> {
        self.saves.get(npc).and_then(|n| n.map.at(&["traits", "shop", "rightClickShop"]).and_then(|v| v.text())).filter(|s| !s.is_empty())
    }

    fn storage_of(&self, src: &Src) -> Storage {
        match src {
            Src::Shop(t) => self.shop_ref(t).map(|s| s.storage.clone()).unwrap_or_default(),
            Src::Trait(id) => self
                .saves
                .get(*id)
                .and_then(|n| n.map.at(&["traits", "shop", "storage"]).map(Storage::from_node))
                .unwrap_or_default(),
        }
    }

    fn set_storage(&mut self, src: &Src, st: Storage) {
        match src {
            Src::Shop(t) => {
                if let Some(s) = self.shop_mut(t) {
                    s.storage = st;
                    self.shops_changed();
                }
            }
            Src::Trait(id) => {
                if let Some(n) = self.saves.get_mut(*id) {
                    n.map.put(&["traits", "shop", "storage"], st.to_node());
                    n.add_trait_name("shop");
                    self.mark_dirty();
                }
            }
        }
    }

    /// Every permission node a shop interaction may ask the host about: the view permissions, the global one,
    /// the nodes of permission actions and the editor nodes. The host fills [`ShopPlayer::perms`] from it.
    pub fn shop_wanted_nodes(&self) -> Vec<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        if !self.cfg.shops.global_view_permission.is_empty() {
            out.insert(self.cfg.shops.global_view_permission.clone());
        }
        for s in self.shops.npc.values().chain(self.shops.global.values()) {
            if let Some(p) = &s.view_permission {
                out.insert(p.clone());
            }
            for page in &s.pages {
                for it in page.items.values() {
                    for a in it.cost.iter().chain(it.result.iter()) {
                        if let Action::Permission { permissions } = a {
                            out.extend(permissions.iter().cloned());
                        }
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    /// Whether any shop is stored (the host skips reading players otherwise).
    pub fn has_shops(&self) -> bool {
        !self.shops.npc.is_empty() || !self.shops.global.is_empty()
    }

    /// Whether this player has a shop menu open.
    pub fn shop_session_open(&self, player: u128) -> bool {
        self.shop_ui.sessions.contains_key(&player)
    }

    fn buyer_of(&self, player: u128) -> Buyer {
        let p = self.shop_player.get(&player).cloned().unwrap_or_default();
        let mut inv = p.inv;
        inv.resize(36, None);
        Buyer {
            uuid: player,
            name: if p.name.is_empty() { self.player_name(player) } else { p.name },
            world: p.world,
            levels: p.levels,
            inv,
            perms: p.perms,
            op: p.op,
        }
    }

    /// `NPCShop.canView` for a player.
    fn shop_viewable(&self, t: &Target, player: u128) -> bool {
        let Some(shop) = self.shop_ref(t) else { return false };
        let b = self.buyer_of(player);
        shop.can_view(&self.cfg.shops.global_view_permission, &|n| b.op || b.perms.contains(n))
    }

    fn can_use(&self, s: &Session, node: &str) -> bool {
        match &s.sender {
            Some(se) => commands::allowed(se, self.cfg.command_level, node),
            None => false,
        }
    }

    fn tell_raw(&mut self, player: u128, text: &str) {
        let legacy = commands::tags(text, "&f");
        self.ops.push(Op::Tell { player, legacy });
    }

    fn tell_msg(&mut self, player: u128, text: &str) {
        let legacy = self.cfg.messages.format(&commands::tags(text, "&a"));
        self.ops.push(Op::Tell { player, legacy });
    }

    fn tell_err(&mut self, player: u128, text: &str) {
        let legacy = self.cfg.messages.error(text);
        self.ops.push(Op::Tell { player, legacy });
    }

    // ---- opening ---------------------------------------------------------------------

    /// `NPCShop.display(storage, player)`: nothing when the player may not view it, an error for an empty shop.
    pub fn shop_display(&mut self, player: u128, shop: Target, src: Src) {
        let Some(sh) = self.shop_ref(&shop) else { return };
        if !self.shop_viewable(&shop, player) {
            return;
        }
        if sh.pages.is_empty() {
            self.tell_err(player, "Empty shop");
            return;
        }
        if sh.shop_type == shop::ShopType::Trader {
            self.open_trader(player, &shop);
            return;
        }
        let s = Session {
            player,
            sender: None,
            npc: None,
            window: None,
            title: String::new(),
            rows: 0,
            acts: Vec::new(),
            stack: vec![Screen::Viewer { shop, src, page: 0, last: None }],
            prompting: false,
            dead: false,
        };
        self.start_session(s);
    }

    /// `NPCTraderShopViewer`: a merchant screen with one trade per shop item that has an item cost. The
    /// payments are the first two stacks of the item actions of the cost, the result is the display item.
    /// The game's own trade rules take the payments and give the result (the original cancels the result
    /// click and runs the purchase itself: limits, commands and the other actions do not run here).
    fn open_trader(&mut self, player: u128, target: &Target) {
        let Some(sh) = self.shop_ref(target) else { return };
        let b = self.buyer_of(player);
        let snap = self.shop_snap();
        let mut trades = Vec::new();
        for pg in &sh.pages {
            for it in pg.items.values() {
                let Some(result) = it.display_item(Some(&b), &self.cfg.shops, &snap) else { continue };
                let mut costs: Vec<StackData> = Vec::new();
                for a in &it.cost {
                    if let shop::Action::Item { items, .. } = a {
                        for st in items {
                            if costs.len() < 2 {
                                costs.push(st.clone());
                            }
                        }
                    }
                }
                if !costs.is_empty() {
                    trades.push(Trade { costs, result });
                }
            }
        }
        let title = if sh.title().is_empty() { "Shop".to_owned() } else { shop::player_placeholders(&sh.title(), &b) };
        let title = commands::tags(&title, "&0");
        self.merchants.remove(&player);
        self.ops.push(Op::OpenMerchant { player, title, trades });
    }

    /// The merchant screen opened (`None`: a mod cancelled it).
    pub fn merchant_opened(&mut self, player: u128, window: Option<i32>) {
        match window {
            Some(_) => {
                self.merchants.insert(player, (window, None));
            }
            None => {
                self.merchants.remove(&player);
            }
        }
    }

    /// `merchant_trade_selected`: the player picked trade `index` in the screen `window`.
    pub fn merchant_selected(&mut self, player: u128, window: i32, index: i32) {
        if let Some(m) = self.merchants.get_mut(&player)
            && m.0 == Some(window)
        {
            m.1 = usize::try_from(index).ok();
        }
    }

    /// The trade a player has selected in the trader screen they have open.
    pub fn merchant_trade(&self, player: u128) -> Option<usize> {
        self.merchants.get(&player).and_then(|m| m.1)
    }

    /// `NPCShop.displayEditor(trait, player)`.
    pub fn shop_open_editor(&mut self, player: u128, shop: Target, npc: Option<i32>, sender: Sender) {
        if self.shop_ref(&shop).is_none() {
            return;
        }
        let s = Session {
            player,
            sender: Some(sender),
            npc,
            window: None,
            title: String::new(),
            rows: 0,
            acts: Vec::new(),
            stack: vec![Screen::Settings { shop }],
            prompting: false,
            dead: false,
        };
        self.start_session(s);
    }

    fn start_session(&mut self, mut s: Session) {
        let player = s.player;
        self.menus.remove(&player);
        self.shop_ui.prompts.remove(&player);
        self.shop_ui.sessions.remove(&player);
        self.present(&mut s, true);
        self.shop_ui.sessions.insert(player, s);
    }

    /// `ShopTrait.onRightClick`: the NPC's `rightClickShop` (its own shop when that name is not a global one).
    pub fn shop_right_click(&mut self, player: u128, npc: i32) {
        let Some(name) = self.shop_right_click_name(npc) else { return };
        let global = self.cfg.shops.global_view_permission.clone();
        let b = self.buyer_of(player);
        if !global.is_empty() && !(b.op || b.perms.contains(&global)) {
            return;
        }
        let target = match self.shops.global.get(&name) {
            Some(_) => Target::Global(name),
            None => match self.shop_default(npc) {
                Some(t) => t,
                None => return,
            },
        };
        self.shop_display(player, target, Src::Trait(npc));
    }

    /// Open-shop actions run a tick later; stale sessions and prompts of players who left go.
    pub(super) fn shop_tick(&mut self) {
        if !self.shop_ui.pending.is_empty() {
            let now = self.tick;
            let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.shop_ui.pending).into_iter().partition(|(t, _, _)| *t <= now);
            self.shop_ui.pending = wait;
            for (_, player, name) in due {
                if let Some(t) = self.shop_find(&name) {
                    let src = Src::Shop(t.clone());
                    self.shop_display(player, t, src);
                }
            }
        }
        if self.tick % 100 == 0 {
            let online: BTreeSet<u128> = self.online.iter().map(|p| p.uuid).collect();
            if !online.is_empty() || self.tick > 0 {
                self.shop_ui.sessions.retain(|p, s| online.contains(p) || s.prompting);
                let tick = self.tick;
                self.shop_ui.prompts.retain(|p, (_, t)| online.contains(p) && tick.saturating_sub(*t) < PROMPT_TICKS);
            }
        }
    }

    // ---- rendering -------------------------------------------------------------------

    fn render(&self, s: &Session) -> Option<Rendered> {
        let top = s.stack.last()?;
        match top {
            Screen::Viewer { shop, src: _, page, last: _ } => self.render_viewer(s.player, shop, *page),
            Screen::Settings { shop } => self.render_settings(s, shop),
            Screen::Contents { shop, page, copying: _ } => self.render_contents(shop, *page),
            Screen::PageSettings { shop, page, .. } => {
                let sh = self.shop_ref(shop)?;
                let title = sh.pages.get(*page).and_then(|p| p.title.clone());
                let mut r = Rendered::new("NPC Shop Page Editor", 5);
                r.put(4, icon("minecraft:feather", "&fSet page title", &[format!("Currently: {}", java_null(&title))]), Act::PTitle);
                r.put(40, icon("minecraft:tnt", "&cRemove page", &[]), Act::PRemove);
                Some(r)
            }
            Screen::ItemEditor { modified, .. } => Some(self.render_item_editor(modified)),
            Screen::List { work, op_toggle: _, .. } => {
                let (title, rows, values, noun) = match work {
                    Action::Command { commands, .. } => ("Command editor", 4u8, commands.clone(), "command"),
                    Action::Permission { permissions } => ("Permissions editor", 3u8, permissions.clone(), "permission"),
                    _ => return None,
                };
                let mut r = Rendered::new(title, rows);
                for i in 0..(usize::from(rows) * 9).min(27) {
                    if let Some(v) = values.get(i) {
                        r.put(
                            i,
                            icon("minecraft:feather", &format!("&fSet {noun}"), &["Right click to remove".to_owned(), format!("Currently: {v}")]),
                            Act::LSlot(i),
                        );
                    } else {
                        r.act(i, Act::LSlot(i));
                    }
                }
                if let Action::Command { op, server, .. } = work {
                    r.put(30, icon("minecraft:command_block", "Run commands as server", &[on_off(*server)]), Act::LServer);
                    r.put(31, icon("minecraft:comparator", "Run commands as op", &[on_off(*op)]), Act::LOp);
                }
                Some(r)
            }
            Screen::Items { work, slots, .. } => {
                let Action::Item { meta_filter, require_undamaged, .. } = work else { return None };
                let mut r = Rendered::new("Item editor", 4);
                for i in 0..27 {
                    match slots.get(i).and_then(|s| s.clone()) {
                        Some(st) => r.put(i, st, Act::ItemsSlot(i)),
                        None => r.act(i, Act::ItemsSlot(i)),
                    }
                }
                r.put(28, icon("minecraft:anvil", "Must have no damage", &[on_off(*require_undamaged)]), Act::IUndamaged);
                r.put(29, icon("minecraft:book", "Component comparison filter", &meta_filter.clone()), Act::IFilter);
                Some(r)
            }
            Screen::Picker { shop } => {
                let sh = self.shop_ref(shop)?;
                let mut r = Rendered::new("Set shop type", 2);
                let desc = |t: ShopType| match t {
                    ShopType::Default => "Default (5x9 chest)",
                    ShopType::Chest4x9 => "4x9 chest",
                    ShopType::Chest3x9 => "3x9 chest",
                    ShopType::Chest2x9 => "2x9 chest",
                    ShopType::Chest1x9 => "1x9 chest",
                    ShopType::Trader => "Trader",
                };
                for (i, t) in ShopType::ALL.iter().enumerate() {
                    let name = t.name();
                    let pretty = format!("{}{}", &name[..1], name[1..].to_ascii_lowercase());
                    let colour = if sh.shop_type == *t { "&a" } else { "&c" };
                    let material = if *t == ShopType::Trader { "minecraft:emerald" } else { "minecraft:chest" };
                    r.put(i * 2, icon(material, &format!("{colour}{pretty}"), &[desc(*t).to_owned()]), Act::Pick(*t));
                }
                Some(r)
            }
            Screen::Storage { src, slots } => {
                let st = self.storage_of(src);
                let mut r = Rendered::new("Item storage", 4);
                for i in 0..27 {
                    match slots.get(i).and_then(|s| s.clone()) {
                        Some(stack) => r.put(i, stack, Act::StSlot(i)),
                        None => r.act(i, Act::StSlot(i)),
                    }
                }
                r.put(28, icon("minecraft:beacon", "Unlimited", &[on_off(st.unlimited)]), Act::StUnlimited);
                let limit = if st.inventory_size_limit == -1 { "&aUnlimited".to_owned() } else { format!("&e{}", st.inventory_size_limit) };
                r.put(29, icon("minecraft:comparator", "Inventory size limit", &[limit]), Act::StLimit);
                Some(r)
            }
        }
    }

    fn render_viewer(&self, player: u128, target: &Target, page: usize) -> Option<Rendered> {
        let sh = self.shop_ref(target)?;
        let ty = sh.shop_type;
        let pg = sh.pages.get(page)?;
        let b = self.buyer_of(player);
        let snap = self.shop_snap();
        let title = match &pg.title {
            Some(t) if !t.is_empty() => shop::player_placeholders(t, &b),
            _ => {
                if sh.title().is_empty() {
                    "Shop".to_owned()
                } else {
                    shop::player_placeholders(&sh.title(), &b)
                }
            }
        };
        let mut r = Rendered::new(&commands::tags(&title, "&0"), ty.rows());
        for (slot, it) in &pg.items {
            if *slot < r.items.len() {
                r.items[*slot] = it.display_item(Some(&b), &self.cfg.shops, &snap);
                r.acts[*slot] = Act::ViewerItem(*slot);
            }
        }
        let feather = || StackData::new("minecraft:feather", 1);
        if page > 0 {
            let at = ty.prev_slot();
            let mut st = pg.items.get(&at).and_then(|i| i.display_item(Some(&b), &self.cfg.shops, &snap)).unwrap_or_else(feather);
            st.meta.display_name = Some(format!("Previous page ({page})"));
            r.put(at, st, Act::Prev);
        }
        if page + 1 < sh.pages.len() {
            let at = ty.next_slot();
            let mut st = pg.items.get(&at).and_then(|i| i.display_item(Some(&b), &self.cfg.shops, &snap)).unwrap_or_else(feather);
            st.meta.display_name = Some(format!("Next page ({})", page + 1));
            r.put(at, st, Act::Next);
        }
        Some(r)
    }

    fn render_contents(&self, target: &Target, page: usize) -> Option<Rendered> {
        let sh = self.shop_ref(target)?;
        let ty = sh.shop_type;
        let total = sh.pages.len().max(page + 1);
        let mut r = Rendered::new(&format!("NPC Shop Contents Editor ({}/{})", page + 1, total + 1), ty.rows());
        let snap = self.shop_snap();
        let empty = ShopPage::default();
        let pg = sh.pages.get(page).unwrap_or(&empty);
        for i in 0..ty.size() {
            match pg.items.get(&i).and_then(|it| it.display_item(None, &self.cfg.shops, &snap)) {
                Some(st) => r.put(i, st, Act::CSlot(i)),
                None => r.act(i, Act::CSlot(i)),
            }
        }
        let feather = || StackData::new("minecraft:feather", 1);
        let item_or_feather = |at: usize| pg.items.get(&at).and_then(|it| it.display_item(None, &self.cfg.shops, &snap)).unwrap_or_else(feather);
        if page > 0 {
            let at = ty.prev_slot();
            let mut st = item_or_feather(at);
            st.meta.display_name = Some(format!("Previous page ({page})"));
            r.put(at, st, Act::CPrev);
        }
        let at = ty.next_slot();
        let mut st = item_or_feather(at);
        st.meta.display_name = Some(if page + 1 >= sh.pages.len() { "New page".to_owned() } else { format!("Next page ({})", page + 1) });
        r.put(at, st, Act::CNext);
        r.put(ty.edit_slot(), icon("minecraft:book", "Edit page", &[]), Act::CEdit);
        Some(r)
    }

    fn render_settings(&self, s: &Session, target: &Target) -> Option<Rendered> {
        let sh = self.shop_ref(target)?;
        let mut r = Rendered::new("NPC Shop Editor", 2);
        r.put(0, icon("minecraft:oak_sign", "&fEdit permission required to view shop", &[java_null(&sh.view_permission)]), Act::SPerm);
        r.put(2, icon("minecraft:feather", "&fEdit shop items", &[]), Act::SItems);
        r.put(4, icon("minecraft:name_tag", "&fEdit shop title", &[sh.title()]), Act::STitle);
        let mut right = icon("minecraft:command_block", "Command Block", &[]);
        let mut ty = icon("minecraft:chest", "&fSet shop type", &[]);
        if let Some(npc) = s.npc {
            let on = self.shop_right_click_name(npc).as_deref() == Some(sh.name.as_str());
            right = icon("minecraft:command_block", "&fShow shop on right click", &[on.to_string()]);
            ty = icon("minecraft:chest", "&fSet shop type", &[sh.shop_type.name().to_owned()]);
        }
        r.put(6, right, Act::SRight);
        r.put(8, ty, Act::SType);
        r.put(10, icon("minecraft:ender_chest", "&fView inventory", &[]), Act::SStorage);
        Some(r)
    }

    fn render_item_editor(&self, m: &ShopItem) -> Rendered {
        let snap = self.shop_snap();
        let mut r = Rendered::new("NPC Shop Item Editor", 6);
        if let Some(d) = m.display_item(None, &self.cfg.shops, &snap) {
            r.put(40, d, Act::IDisplay);
        } else {
            r.act(40, Act::IDisplay);
        }
        let limit = |n: i32| format!("Times purchasable: {n}{}", if n == 0 { " (no limit)" } else { "" });
        r.put(43, icon("minecraft:apple", "Reset purchase history", &[format!("{} purchases", m.purchases.len())]), Act::IReset);
        r.put(33, icon("minecraft:egg", "Number of purchases limit per player", &[limit(m.times_purchasable)]), Act::ITimes);
        r.put(42, icon("minecraft:egg", "Global number of purchases limit", &[limit(m.global_times_purchasable)]), Act::IGlobal);
        // Two of the original's slots are set twice (the already-purchased and unsuccessful messages are
        // overwritten by the clickable and visible expressions): the later buttons are what shows.
        r.put(38, icon("minecraft:anvil", "Set clickable expression", &[format!("Currently: {}", java_null(&m.is_clickable_expression))]), Act::IClickable);
        r.put(30, icon("minecraft:green_wool", "Set successful click message, currently:", &[unset(&m.result_message)]), Act::IResult);
        r.put(29, icon("minecraft:beacon", "Set visibility expression", &[format!("Currently: {}", java_null(&m.is_visible_expression))]), Act::IVisible);
        r.put(
            32,
            icon(
                "minecraft:feather",
                "Set click to confirm message.",
                &[
                    "For example, 'click again to buy this item'".to_owned(),
                    "You can use <cost> or <result> placeholders.".to_owned(),
                    "Currently:".to_owned(),
                    unset(&m.click_to_confirm_message),
                ],
            ),
            Act::IConfirm,
        );
        r.put(
            31,
            icon("minecraft:redstone", "Sell as many times as possible on shift click", &[format!("Currently: {}", m.max_repeats_on_shift_click)]),
            Act::IShift,
        );
        r.put(39, icon("minecraft:name_tag", "&fSet name", &[]), Act::IName);
        r.put(41, icon("minecraft:book", "&fSet description", &[]), Act::IDesc);
        r.put(48, icon("minecraft:redstone_block", "&7Cancel", &[]), Act::ICancel);
        r.put(49, icon("minecraft:tnt", "&cRemove", &[]), Act::IRemove);
        r.put(50, icon("minecraft:emerald_block", "&aSave", &[]), Act::ISave);
        let mut pos = 0usize;
        for kind in Kind::ALL {
            let Some((id, name)) = kind.button() else { continue };
            let (row, col) = (pos / 3, pos % 3);
            for (cost, list, base_col, suffix) in [(true, &m.cost, 0usize, "Cost"), (false, &m.result, 6usize, "Result")] {
                let old = list.iter().find(|a| a.kind() == kind);
                let lore = old.map(|a| lines(&a.describe(&snap))).unwrap_or_default();
                r.put(row * 9 + base_col + col, icon(id, &format!("{name} {suffix}"), &lore), Act::IAction(cost, kind));
            }
            pos += 1;
        }
        r
    }

    /// Shows the top screen: the items change in place when the title and size stay, else the menu is opened again.
    fn present(&mut self, s: &mut Session, force_open: bool) {
        let Some(r) = self.render(s) else {
            s.dead = true;
            if s.window.is_some() {
                self.ops.push(Op::CloseMenu { player: s.player });
            }
            return;
        };
        let same = s.window.is_some() && s.title == r.title && s.rows == r.rows;
        if same && !force_open {
            if let Some(window) = s.window {
                self.ops.push(Op::SetMenuItems { player: s.player, window, items: r.items.clone() });
            }
        } else {
            self.ops.push(Op::OpenMenu { player: s.player, title: r.title.clone(), rows: r.rows, items: r.items.clone(), in_place: s.window.is_some() });
            s.window = None;
        }
        s.title = r.title;
        s.rows = r.rows;
        s.acts = r.acts;
    }

    // ---- clicks ----------------------------------------------------------------------

    /// A click in a shop menu. `None` when the player has none.
    pub(super) fn shop_click(&mut self, player: u128, window: i32, slot: i32, button: i32, shift: bool) -> Option<MenuAction> {
        let mut s = self.shop_ui.sessions.remove(&player)?;
        let out = if s.window != Some(window) { MenuAction::Nothing } else { self.click_inner(&mut s, slot, button, shift) };
        if !s.dead {
            self.shop_ui.sessions.insert(player, s);
        }
        Some(out)
    }

    fn click_inner(&mut self, s: &mut Session, slot: i32, button: i32, shift: bool) -> MenuAction {
        let size = usize::from(s.rows) * 9;
        let Ok(slot) = usize::try_from(slot) else { return MenuAction::Nothing };
        if slot >= size + 36 {
            return MenuAction::Nothing;
        }
        if slot >= size {
            let p = slot - size;
            let inv_slot = (if p < 27 { 9 + p } else { p - 27 }) as u32;
            return match s.stack.last() {
                Some(Screen::Storage { .. }) => MenuAction::Deposit { inv_slot },
                Some(Screen::ItemEditor { .. }) | Some(Screen::Items { .. }) => MenuAction::Read { inv_slot },
                _ => MenuAction::Nothing,
            };
        }
        let act = s.acts.get(slot).cloned().unwrap_or(Act::None);
        self.act(s, act, button, shift)
    }

    fn act(&mut self, s: &mut Session, act: Act, button: i32, shift: bool) -> MenuAction {
        let player = s.player;
        let right = button == 1;
        match act {
            Act::None => {}
            Act::Prev | Act::Next => {
                if let Some(Screen::Viewer { page, .. }) = s.stack.last_mut() {
                    *page = if act == Act::Prev { page.saturating_sub(1) } else { *page + 1 };
                }
                self.present(s, false);
            }
            Act::ViewerItem(slot) => self.viewer_buy(s, slot, shift),
            // ---- settings
            Act::SPerm => self.prompt(s, PromptFor::ShopPerm, "Enter the permission required to view the shop"),
            Act::STitle => self.prompt(s, PromptFor::ShopTitle, "Enter the shop title"),
            Act::SItems => {
                if let Some(Screen::Settings { shop }) = s.stack.last().cloned() {
                    s.stack.push(Screen::Contents { shop, page: 0, copying: None });
                    self.present(s, false);
                }
            }
            Act::SRight => {
                if !self.can_use(s, "citizens.npc.shop.editor.show-on-right-click") {
                    self.tell_err(player, NO_PERMISSION);
                    return MenuAction::Nothing;
                }
                let (Some(Screen::Settings { shop }), Some(npc)) = (s.stack.last().cloned(), s.npc) else { return MenuAction::Nothing };
                let Some(name) = self.shop_ref(&shop).map(|sh| sh.name.clone()) else { return MenuAction::Nothing };
                self.shop_ensure_trait(npc);
                let on = self.shop_right_click_name(npc).as_deref() == Some(name.as_str());
                if let Some(n) = self.saves.get_mut(npc) {
                    if on {
                        n.map.drop_path(&["traits", "shop", "rightClickShop"]);
                    } else {
                        n.map.put(&["traits", "shop", "rightClickShop"], crate::horizon_citizens_core::yaml::Node::str(&name));
                    }
                }
                self.mark_dirty();
                self.present(s, false);
            }
            Act::SType => {
                if !self.can_use(s, "citizens.npc.shop.editor.set-shop-type") {
                    self.tell_err(player, NO_PERMISSION);
                    return MenuAction::Nothing;
                }
                if let Some(Screen::Settings { shop }) = s.stack.last().cloned() {
                    s.stack.push(Screen::Picker { shop });
                    self.present(s, false);
                }
            }
            Act::SStorage => {
                let src = match (s.npc, s.stack.last()) {
                    (Some(npc), Some(Screen::Settings { .. })) => Src::Trait(npc),
                    (None, Some(Screen::Settings { shop })) => Src::Shop(shop.clone()),
                    _ => return MenuAction::Nothing,
                };
                let st = self.storage_of(&src);
                let mut slots: Vec<Option<StackData>> = st.inventory.iter().take(27).cloned().map(Some).collect();
                slots.resize(27, None);
                s.stack.push(Screen::Storage { src, slots });
                self.present(s, false);
            }
            Act::Pick(t) => {
                if let Some(Screen::Picker { shop }) = s.stack.last().cloned() {
                    if let Some(sh) = self.shop_mut(&shop) {
                        sh.shop_type = t;
                    }
                    self.shops_changed();
                    self.back(s);
                }
            }
            // ---- contents
            Act::CPrev | Act::CNext | Act::CEdit => {
                let Some(Screen::Contents { shop, page, .. }) = s.stack.last().cloned() else { return MenuAction::Nothing };
                let Some(ty) = self.shop_ref(&shop).map(|sh| sh.shop_type) else { return MenuAction::Nothing };
                if shift {
                    let at = match act {
                        Act::CPrev => ty.prev_slot(),
                        Act::CNext => ty.next_slot(),
                        _ => ty.edit_slot(),
                    };
                    return self.act(s, Act::CSlot(at), button, false);
                }
                match act {
                    Act::CEdit => {
                        if let Some(sh) = self.shop_mut(&shop) {
                            sh.page_mut(page);
                        }
                        s.stack.push(Screen::PageSettings { shop, page, remove: false });
                    }
                    _ => {
                        let np = if act == Act::CPrev { page.saturating_sub(1) } else { page + 1 };
                        if let Some(sh) = self.shop_mut(&shop) {
                            sh.page_mut(np);
                        }
                        self.shops_changed();
                        if let Some(Screen::Contents { page, .. }) = s.stack.last_mut() {
                            *page = np;
                        }
                    }
                }
                self.present(s, false);
            }
            Act::CSlot(i) => self.contents_slot(s, i, right, shift),
            Act::PTitle => self.prompt(s, PromptFor::PageTitle, "Enter the page title"),
            Act::PRemove => {
                if let Some(Screen::PageSettings { remove, .. }) = s.stack.last_mut() {
                    *remove = true;
                }
                self.back(s);
            }
            // ---- item editor
            Act::IDisplay => {
                if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut() {
                    modified.set_display_item(None);
                }
                self.present(s, false);
            }
            Act::IReset => {
                if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut() {
                    modified.reset_purchase_history();
                }
                self.present(s, false);
            }
            Act::IShift => {
                if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut() {
                    modified.max_repeats_on_shift_click = !modified.max_repeats_on_shift_click;
                }
                self.present(s, false);
            }
            Act::ITimes => self.prompt(s, PromptFor::ItemTimes, "Enter the number of purchases allowed per player (0 for no limit)"),
            Act::IGlobal => self.prompt(s, PromptFor::ItemGlobal, "Enter the number of purchases allowed in total (0 for no limit)"),
            Act::IResult => self.prompt(s, PromptFor::ItemResult, "Enter the new successful click message"),
            Act::IConfirm => self.prompt(s, PromptFor::ItemConfirm, "Enter the new click to confirm message"),
            Act::IVisible => self.prompt(s, PromptFor::ItemVisible, "Enter the visibility expression"),
            Act::IClickable => self.prompt(s, PromptFor::ItemClickable, "Enter the clickable expression"),
            Act::IName => {
                if matches!(s.stack.last(), Some(Screen::ItemEditor { modified, .. }) if modified.display.is_some()) {
                    self.prompt(s, PromptFor::ItemName, "Enter the new item name");
                }
            }
            Act::IDesc => {
                if matches!(s.stack.last(), Some(Screen::ItemEditor { modified, .. }) if modified.display.is_some()) {
                    self.prompt(s, PromptFor::ItemDescription, "Type the new item description");
                }
            }
            Act::ICancel => self.back(s),
            Act::IRemove => {
                if let Some(Screen::ItemEditor { base, .. }) = s.stack.last_mut() {
                    *base = None;
                }
                self.back(s);
            }
            Act::ISave => {
                if let Some(Screen::ItemEditor { base, modified, .. }) = s.stack.last_mut() {
                    *base = Some(modified.clone());
                }
                self.back(s);
            }
            Act::IAction(cost, kind) => self.open_action_editor(s, cost, kind),
            // ---- list editors
            Act::LSlot(i) => {
                let Some(Screen::List { work, .. }) = s.stack.last_mut() else { return MenuAction::Nothing };
                let list = match work {
                    Action::Command { commands, .. } => commands,
                    Action::Permission { permissions } => permissions,
                    _ => return MenuAction::Nothing,
                };
                if right {
                    if i < list.len() {
                        list.remove(i);
                    }
                    self.present(s, false);
                } else {
                    let noun = if matches!(s.stack.last(), Some(Screen::List { work: Action::Command { .. }, .. })) { "command" } else { "permission" };
                    self.prompt(s, PromptFor::ListValue(i), &format!("Enter the {noun}"));
                }
            }
            Act::LServer => {
                if let Some(Screen::List { work: Action::Command { server, .. }, .. }) = s.stack.last_mut() {
                    *server = !*server;
                }
                self.present(s, false);
            }
            Act::LOp => {
                if let Some(Screen::List { work: Action::Command { op, .. }, op_toggle, .. }) = s.stack.last_mut() {
                    // `InputMenus.clickToggle(.., base.server)`: the toggle starts from `server`, not `op`.
                    *op_toggle = !*op_toggle;
                    *op = *op_toggle;
                }
                self.present(s, false);
            }
            Act::ItemsSlot(i) => {
                if let Some(Screen::Items { slots, .. }) = s.stack.last_mut()
                    && i < slots.len()
                {
                    slots[i] = None;
                }
                self.present(s, false);
            }
            Act::IUndamaged => {
                if let Some(Screen::Items { work: Action::Item { require_undamaged, .. }, .. }) = s.stack.last_mut() {
                    *require_undamaged = !*require_undamaged;
                }
                self.present(s, false);
            }
            Act::IFilter => self.prompt(s, PromptFor::ItemsFilter, "Enter the component comparison filter (comma separated paths)"),
            // ---- storage
            Act::StUnlimited => {
                let Some(Screen::Storage { src, .. }) = s.stack.last().cloned() else { return MenuAction::Nothing };
                let mut st = self.storage_of(&src);
                st.unlimited = !st.unlimited;
                self.set_storage(&src, st);
                self.present(s, false);
            }
            Act::StLimit => self.prompt(s, PromptFor::StorageLimit, "Enter the inventory size limit (-1 for unlimited)"),
            Act::StSlot(i) => {
                if let Some(Screen::Storage { slots, .. }) = s.stack.last_mut()
                    && let Some(st) = slots.get_mut(i).and_then(Option::take)
                {
                    self.present(s, false);
                    return MenuAction::Take(st);
                }
            }
        }
        MenuAction::Nothing
    }

    /// A click on a slot of the contents editor.
    fn contents_slot(&mut self, s: &mut Session, i: usize, right: bool, shift: bool) {
        let Some(Screen::Contents { shop, page, copying }) = s.stack.last().cloned() else { return };
        let existing = self.shop_ref(&shop).and_then(|sh| sh.pages.get(page)).and_then(|p| p.items.get(&i)).cloned();
        match existing {
            Some(item) if shift && item.display.is_some() => {
                // Shift picks the item up: a left click moves it, a right click copies it.
                if !right
                    && let Some(sh) = self.shop_mut(&shop)
                {
                    sh.page_mut(page).items.remove(&i);
                    self.shops_changed();
                }
                if let Some(Screen::Contents { copying, .. }) = s.stack.last_mut() {
                    *copying = Some(item);
                }
                self.present(s, false);
            }
            Some(item) => {
                s.stack.push(Screen::ItemEditor { shop, page, slot: i, base: Some(item.clone()), modified: item });
                self.present(s, false);
            }
            None => {
                if let Some(c) = copying {
                    if let Some(sh) = self.shop_mut(&shop) {
                        sh.page_mut(page).items.insert(i, c);
                    }
                    self.shops_changed();
                    if let Some(Screen::Contents { copying, .. }) = s.stack.last_mut() {
                        *copying = None;
                    }
                    self.present(s, false);
                    return;
                }
                let fresh = ShopItem::new(&self.cfg.shops);
                s.stack.push(Screen::ItemEditor { shop, page, slot: i, base: Some(fresh.clone()), modified: fresh });
                self.present(s, false);
            }
        }
    }

    fn open_action_editor(&mut self, s: &mut Session, cost: bool, kind: Kind) {
        let player = s.player;
        if !self.can_use(s, "citizens.admin") && !self.can_use(s, kind.edit_node()) {
            self.tell_err(player, NO_PERMISSION);
            return;
        }
        let Some(Screen::ItemEditor { modified, .. }) = s.stack.last() else { return };
        let list = if cost { &modified.cost } else { &modified.result };
        let existing = list.iter().find(|a| a.kind() == kind).cloned();
        let work = existing.unwrap_or_else(|| Action::new(kind));
        match kind {
            Kind::Item => {
                let Action::Item { items, .. } = &work else { return };
                let mut slots: Vec<Option<StackData>> = items.iter().take(27).cloned().map(Some).collect();
                slots.resize(27, None);
                s.stack.push(Screen::Items { cost, work, slots });
                self.present(s, false);
            }
            Kind::Command | Kind::Permission => {
                let op_toggle = matches!(&work, Action::Command { server: true, .. });
                s.stack.push(Screen::List { cost, work, op_toggle });
                self.present(s, false);
            }
            Kind::Experience => self.prompt(s, PromptFor::ActExp(cost), "Enter the experience levels"),
            Kind::OpenShop => self.prompt(s, PromptFor::ActOpenShop(cost), "Enter the name of the shop to open"),
            Kind::Condition => self.prompt(s, PromptFor::ActCondition(cost), "Enter the condition"),
            Kind::Money => {}
        }
    }

    /// A purchase: the click on a shop item of the viewer.
    fn viewer_buy(&mut self, s: &mut Session, slot: usize, shift: bool) {
        let player = s.player;
        let Some(Screen::Viewer { shop, src, page, last }) = s.stack.last().cloned() else { return };
        let buyer = self.buyer_of(player);
        let snap = self.shop_snap();
        let mut storage = self.storage_of(&src);
        let second = last == Some((page, slot));
        let outcome: Option<Outcome> = match self.shop_mut(&shop).and_then(|sh| sh.pages.get_mut(page)).and_then(|p| p.items.get_mut(&slot)) {
            Some(item) => Some(item.click(&mut storage, &buyer, shift, second, &snap)),
            None => None,
        };
        if let Some(Screen::Viewer { last, .. }) = s.stack.last_mut() {
            *last = Some((page, slot));
        }
        let Some(o) = outcome else { return };
        self.set_storage(&src, storage);
        self.shops_changed();
        self.apply_outcome(player, &buyer, o);
        // An open-shop action closed the menu; else the display shows the new counts.
        if self.shop_ui.pending.iter().any(|(_, p, _)| *p == player) {
            s.dead = true;
            self.ops.push(Op::CloseMenu { player });
            return;
        }
        self.present(s, false);
    }

    /// Runs what a purchase decided: messages, the new levels and inventory, commands, permissions, open-shop.
    fn apply_outcome(&mut self, player: u128, before: &Buyer, o: Outcome) {
        for m in &o.messages {
            self.tell_raw(player, m);
        }
        if o.done {
            let levels = (o.levels != before.levels).then_some(o.levels);
            let mut slots = Vec::new();
            for (i, now) in o.inv.iter().enumerate() {
                if before.inv.get(i).cloned().flatten() != *now {
                    slots.push((i as u32, now.clone()));
                }
            }
            if levels.is_some() || !slots.is_empty() {
                self.ops.push(Op::ShopApply { player, levels, slots });
                if let Some(p) = self.shop_player.get_mut(&player) {
                    p.levels = o.levels;
                    p.inv = o.inv.clone();
                }
            }
        }
        let uuid = crate::horizon_citizens_core::ids::format_uuid(player);
        for e in o.effects {
            match e {
                Effect::Command { line, as_player } => {
                    if !as_player {
                        self.ops.push(Op::Console(line));
                    } else if self.cfg.commands.player_commands_via_execute {
                        self.ops.push(Op::Console(format!("execute as {uuid} at {uuid} run {line}")));
                    } else {
                        eprintln!("[citizens] shop: player command skipped (player-commands-via-execute is off): {line}");
                    }
                }
                Effect::Perm { node, set } => {
                    let line = if set { format!("lp user {uuid} permission set {node} true") } else { format!("lp user {uuid} permission unset {node}") };
                    self.ops.push(Op::Console(line));
                }
                Effect::OpenShop(name) => {
                    self.shop_ui.pending.push((self.tick + 1, player, name));
                }
            }
        }
    }

    // ---- navigation -------------------------------------------------------------------

    /// `transitionBack`: the screen's close callback runs, the one below is shown again.
    fn back(&mut self, s: &mut Session) {
        let Some(top) = s.stack.pop() else { return };
        self.on_close(s, top);
        if s.stack.is_empty() {
            s.dead = true;
            self.ops.push(Op::CloseMenu { player: s.player });
        } else {
            self.present(s, false);
        }
    }

    /// A screen's `onClose` callback.
    fn on_close(&mut self, s: &mut Session, top: Screen) {
        match top {
            Screen::ItemEditor { shop, page, slot, base, .. } => {
                let base = base.filter(|b| b.display.is_some());
                if let Some(sh) = self.shop_mut(&shop) {
                    match base {
                        Some(b) => {
                            sh.page_mut(page).items.insert(slot, b);
                        }
                        None => {
                            sh.page_mut(page).items.remove(&slot);
                        }
                    }
                    self.shops_changed();
                }
            }
            Screen::List { cost, work, .. } => {
                let empty = match &work {
                    Action::Command { commands, .. } => commands.is_empty(),
                    Action::Permission { permissions } => permissions.is_empty(),
                    _ => true,
                };
                let kind = work.kind();
                self.change_parent_action(s, cost, kind, if empty { None } else { Some(work) });
            }
            Screen::Items { cost, mut work, slots } => {
                let items: Vec<StackData> = slots.into_iter().flatten().collect();
                let empty = items.is_empty();
                if let Action::Item { items: it, .. } = &mut work {
                    *it = items;
                }
                self.change_parent_action(s, cost, Kind::Item, if empty { None } else { Some(work) });
            }
            Screen::PageSettings { page, remove: true, .. } => {
                if let Some(Screen::Contents { shop, page: cur, .. }) = s.stack.last_mut() {
                    let shop = shop.clone();
                    *cur = cur.saturating_sub(1);
                    if let Some(sh) = self.shop_mut(&shop) {
                        sh.remove_page(page);
                    }
                    self.shops_changed();
                }
            }
            Screen::Storage { src, slots } => {
                let mut st = self.storage_of(&src);
                st.inventory = slots.into_iter().flatten().collect();
                self.set_storage(&src, st);
            }
            _ => {}
        }
    }

    fn change_parent_action(&mut self, s: &mut Session, cost: bool, kind: Kind, action: Option<Action>) {
        if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut() {
            let list = if cost { &mut modified.cost } else { &mut modified.result };
            ShopItem::change_action(list, kind, action);
        }
    }

    /// The menu was closed (the player pressed escape or something else opened). Prompts keep their session.
    pub(super) fn shop_closed(&mut self, player: u128, window: i32) -> bool {
        let Some(mut s) = self.shop_ui.sessions.remove(&player) else { return false };
        if s.window != Some(window) {
            self.shop_ui.sessions.insert(player, s);
            return true;
        }
        s.window = None;
        if s.prompting {
            self.shop_ui.sessions.insert(player, s);
            return true;
        }
        // Only what lives outside the editors is kept: the storage items.
        if let Some(top @ Screen::Storage { .. }) = s.stack.pop() {
            self.on_close(&mut s, top);
        }
        true
    }

    /// The glue opened the menu: its window id (None when the host refused).
    pub(super) fn shop_opened(&mut self, player: u128, window: Option<i32>) -> bool {
        let Some(s) = self.shop_ui.sessions.get_mut(&player) else { return false };
        match window {
            Some(w) => s.window = Some(w),
            None => {
                self.shop_ui.sessions.remove(&player);
            }
        }
        true
    }

    /// One of the player's own stacks was clicked (the cursor of the original): it becomes the display item
    /// of the item editor, or goes into the first free slot of the item action editor.
    pub fn menu_read(&mut self, player: u128, stack: StackData) {
        let Some(mut s) = self.shop_ui.sessions.remove(&player) else { return };
        match s.stack.last_mut() {
            Some(Screen::ItemEditor { modified, .. }) => modified.set_display_item(Some(stack)),
            Some(Screen::Items { slots, .. }) => {
                if let Some(free) = slots.iter().position(Option::is_none) {
                    slots[free] = Some(stack);
                }
            }
            _ => {}
        }
        self.present(&mut s, false);
        if !s.dead {
            self.shop_ui.sessions.insert(player, s);
        }
    }

    /// A stack moved into the storage viewer; whether there was room.
    pub(super) fn shop_deposit(&mut self, player: u128, stack: StackData) -> Option<bool> {
        let mut s = self.shop_ui.sessions.remove(&player)?;
        let mut taken = false;
        if let Some(Screen::Storage { slots, .. }) = s.stack.last_mut()
            && let Some(free) = slots.iter().position(Option::is_none)
        {
            slots[free] = Some(stack);
            taken = true;
        }
        if taken {
            self.present(&mut s, false);
        }
        self.shop_ui.sessions.insert(player, s);
        Some(taken)
    }

    // ---- chat input ------------------------------------------------------------------

    fn prompt(&mut self, s: &mut Session, what: PromptFor, text: &str) {
        let player = s.player;
        self.tell_msg(player, &format!("{text}. Type it in chat; [[null]] clears it."));
        s.prompting = true;
        s.window = None;
        self.shop_ui.prompts.insert(player, (what, self.tick));
        self.ops.push(Op::CloseMenu { player });
    }

    /// The player's next chat line while a prompt is open: whether it was taken (and so is not public chat).
    pub fn shop_chat(&mut self, player: u128, text: &str) -> bool {
        let Some((what, since)) = self.shop_ui.prompts.get(&player).cloned() else { return false };
        if self.tick.saturating_sub(since) >= PROMPT_TICKS {
            self.shop_ui.prompts.remove(&player);
            return false;
        }
        let Some(mut s) = self.shop_ui.sessions.remove(&player) else {
            self.shop_ui.prompts.remove(&player);
            return false;
        };
        let value = prompt_value(text);
        let done = self.apply_prompt(&mut s, &what, value);
        if done {
            self.shop_ui.prompts.remove(&player);
            s.prompting = false;
            self.present(&mut s, true);
        } else {
            self.tell_err(player, "That is not a valid value, try again.");
        }
        if !s.dead {
            self.shop_ui.sessions.insert(player, s);
        }
        true
    }

    /// Applies a chat answer to the screen that asked; false keeps the prompt.
    fn apply_prompt(&mut self, s: &mut Session, what: &PromptFor, v: Option<String>) -> bool {
        match what {
            PromptFor::ShopPerm | PromptFor::ShopTitle => {
                let Some(Screen::Settings { shop }) = s.stack.last().cloned() else { return true };
                if let Some(sh) = self.shop_mut(&shop) {
                    if *what == PromptFor::ShopPerm {
                        sh.set_permission(v);
                    } else {
                        sh.title = v;
                    }
                }
                self.shops_changed();
                true
            }
            PromptFor::PageTitle => {
                let Some(Screen::PageSettings { shop, page, .. }) = s.stack.last().cloned() else { return true };
                if let Some(sh) = self.shop_mut(&shop) {
                    sh.page_mut(page).title = v.filter(|t| !t.is_empty());
                }
                self.shops_changed();
                true
            }
            PromptFor::ItemTimes | PromptFor::ItemGlobal => {
                let Some(n) = v.as_deref().and_then(|t| t.trim().parse::<i32>().ok()) else { return false };
                if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut() {
                    if *what == PromptFor::ItemTimes {
                        modified.times_purchasable = n;
                    } else {
                        modified.global_times_purchasable = n;
                    }
                }
                true
            }
            PromptFor::ItemResult | PromptFor::ItemConfirm | PromptFor::ItemVisible | PromptFor::ItemClickable => {
                if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut() {
                    match what {
                        PromptFor::ItemResult => modified.result_message = v,
                        PromptFor::ItemConfirm => modified.click_to_confirm_message = v,
                        PromptFor::ItemVisible => modified.is_visible_expression = v,
                        _ => modified.is_clickable_expression = v,
                    }
                }
                true
            }
            PromptFor::ItemName => {
                if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut()
                    && let Some(d) = modified.display.as_mut()
                {
                    d.meta.display_name = v.map(|n| commands::tags(&n, "&f"));
                }
                true
            }
            PromptFor::ItemDescription => {
                if let Some(Screen::ItemEditor { modified, .. }) = s.stack.last_mut()
                    && let Some(d) = modified.display.as_mut()
                {
                    d.meta.lore = match v {
                        None => Vec::new(),
                        Some(t) => lines(&t).iter().map(|l| commands::tags(l, "&f")).collect(),
                    };
                }
                true
            }
            PromptFor::ListValue(i) => {
                let Some(Screen::List { work, .. }) = s.stack.last_mut() else { return true };
                let list = match work {
                    Action::Command { commands, .. } => commands,
                    Action::Permission { permissions } => permissions,
                    _ => return true,
                };
                match v {
                    None => {
                        if *i < list.len() {
                            list.remove(*i);
                        }
                    }
                    Some(t) => {
                        if *i < list.len() {
                            list[*i] = t;
                        } else {
                            list.push(t);
                        }
                    }
                }
                true
            }
            PromptFor::ItemsFilter => {
                if let Some(Screen::Items { work: Action::Item { meta_filter, .. }, .. }) = s.stack.last_mut() {
                    *meta_filter = v.map(|t| t.split(',').map(str::to_owned).collect()).unwrap_or_default();
                }
                true
            }
            PromptFor::StorageLimit => {
                let Some(n) = v.as_deref().and_then(|t| t.trim().parse::<i32>().ok()) else { return false };
                let Some(Screen::Storage { src, .. }) = s.stack.last().cloned() else { return true };
                let mut st = self.storage_of(&src);
                st.inventory_size_limit = n;
                self.set_storage(&src, st);
                true
            }
            PromptFor::ActExp(cost) => {
                // `filteredStringSetter`: a number that is not negative, else the setter refuses.
                let Some(n) = v.as_deref().and_then(|t| t.trim().parse::<i32>().ok()).filter(|n| *n >= 0) else { return false };
                self.change_parent_action(s, *cost, Kind::Experience, Some(Action::Experience { exp: n }));
                true
            }
            PromptFor::ActOpenShop(cost) => {
                // An empty or unknown shop name removes the action.
                let name = v.filter(|n| self.shop_find(n).is_some());
                let action = name.map(|n| Action::OpenShop { shop_name: Some(n) });
                self.change_parent_action(s, *cost, Kind::OpenShop, action);
                true
            }
            PromptFor::ActCondition(cost) => {
                let text = v.unwrap_or_else(|| "null".to_owned());
                let action = Action::Condition { condition: Some(shop::default_markup(&text)) };
                self.change_parent_action(s, *cost, Kind::Condition, Some(action));
                true
            }
        }
    }
}

use crate::horizon_citizens_core::shop::Page as ShopPage;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_citizens_core::config::Config;
    use crate::horizon_citizens_core::saves::Saves;

    fn world() -> World {
        World::new(Config::default(), Saves::empty(), None)
    }

    fn admin() -> Sender {
        Sender { uuid: Some(0xA1), name: "Alex".into(), permission: 4, at: None, grants: Vec::new() }
    }

    fn player(w: &mut World, levels: i32, inv: Vec<(usize, StackData)>) {
        let mut slots = vec![None; 36];
        for (i, s) in inv {
            slots[i] = Some(s);
        }
        w.shop_player.insert(0xA1, ShopPlayer { name: "Alex".into(), world: "world".into(), levels, inv: slots, perms: BTreeSet::new(), op: true });
    }

    fn shop_with_apple_for_xp(w: &mut World) -> Target {
        let mut sh = Shop::new("bank");
        let mut it = ShopItem::new(&w.cfg.shops);
        it.display = Some(StackData::new("minecraft:apple", 1));
        it.cost = vec![Action::Experience { exp: 3 }];
        it.result = vec![Action::Item { items: vec![StackData::new("minecraft:apple", 2)], meta_filter: vec![], require_undamaged: true }];
        sh.page_mut(0).items.insert(4, it);
        sh.title = Some("Bank".into());
        w.shops.global.insert("bank".into(), sh);
        Target::Global("bank".into())
    }

    fn opened(w: &mut World) -> (String, u8, Vec<Option<StackData>>) {
        let ops = w.take_ops();
        let Some(Op::OpenMenu { title, rows, items, .. }) = ops.into_iter().find(|o| matches!(o, Op::OpenMenu { .. })) else { panic!("no menu opened") };
        w.menu_opened(0xA1, Some(5));
        (title, rows, items)
    }

    #[test]
    fn the_viewer_shows_the_items_and_buys_them() {
        let mut w = world();
        player(&mut w, 5, vec![]);
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_display(0xA1, t.clone(), Src::Shop(t));
        let (title, rows, items) = opened(&mut w);
        assert_eq!((title.as_str(), rows), ("Bank", 5));
        assert_eq!(items[4].as_ref().map(|s| s.id.as_str()), Some("minecraft:apple"));
        assert_eq!(w.menu_click(0xA1, 5, 4, 0, false), MenuAction::Nothing);
        let ops = w.take_ops();
        let apply = ops.iter().find_map(|o| if let Op::ShopApply { levels, slots, .. } = o { Some((levels.clone(), slots.clone())) } else { None });
        let (levels, slots) = apply.expect("a purchase");
        assert_eq!(levels, Some(2), "5 levels minus the cost of 3");
        assert_eq!(slots.len(), 1);
        assert_eq!((slots[0].0, slots[0].1.as_ref().map(|s| s.count)), (0, Some(2)));
    }

    #[test]
    fn a_trader_shop_opens_a_merchant_screen_with_the_item_costs() {
        let mut w = world();
        player(&mut w, 5, vec![]);
        let mut sh = Shop::new("smith");
        sh.shop_type = ShopType::Trader;
        sh.title = Some("Smith".into());
        let mut sword = ShopItem::new(&w.cfg.shops);
        sword.display = Some(StackData::new("minecraft:iron_sword", 1));
        sword.cost = vec![Action::Item { items: vec![StackData::new("minecraft:emerald", 5), StackData::new("minecraft:stick", 2), StackData::new("minecraft:dirt", 1)], meta_filter: vec![], require_undamaged: true }];
        sh.page_mut(0).items.insert(0, sword);
        // Paid in experience only: no trade (a merchant screen takes item payments).
        let mut xp = ShopItem::new(&w.cfg.shops);
        xp.display = Some(StackData::new("minecraft:apple", 1));
        xp.cost = vec![Action::Experience { exp: 3 }];
        sh.page_mut(0).items.insert(1, xp);
        w.shops.global.insert("smith".into(), sh);
        let t = Target::Global("smith".into());
        w.shop_display(0xA1, t.clone(), Src::Shop(t));
        let ops = w.take_ops();
        assert!(!ops.iter().any(|o| matches!(o, Op::OpenMenu { .. })), "no chest: {ops:?}");
        let Some(Op::OpenMerchant { player, title, trades }) = ops.into_iter().find(|o| matches!(o, Op::OpenMerchant { .. })) else { panic!("no merchant screen") };
        assert_eq!((player, title.as_str()), (0xA1, "Smith"));
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].result.id, "minecraft:iron_sword");
        let costs: Vec<(&str, i64)> = trades[0].costs.iter().map(|c| (c.id.as_str(), c.count)).collect();
        assert_eq!(costs, [("minecraft:emerald", 5), ("minecraft:stick", 2)], "two payments at most");
        // The screen's id and the player's selection are kept.
        assert_eq!(w.merchant_trade(0xA1), None);
        w.merchant_opened(0xA1, Some(9));
        w.merchant_selected(0xA1, 8, 0);
        assert_eq!(w.merchant_trade(0xA1), None, "another window");
        w.merchant_selected(0xA1, 9, 0);
        assert_eq!(w.merchant_trade(0xA1), Some(0));
        w.merchant_opened(0xA1, None);
        assert_eq!(w.merchant_trade(0xA1), None);
    }

    #[test]
    fn a_viewer_without_the_view_permission_sees_nothing_and_an_empty_shop_is_an_error() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_mut(&t).unwrap().view_permission = Some("bank.view".into());
        w.shop_player.insert(0xA1, ShopPlayer { name: "Alex".into(), ..ShopPlayer::default() });
        w.shop_display(0xA1, t.clone(), Src::Shop(t.clone()));
        assert!(w.take_ops().is_empty());
        w.shop_player.get_mut(&0xA1).unwrap().perms.insert("bank.view".into());
        w.shop_mut(&t).unwrap().pages.clear();
        w.shop_display(0xA1, t.clone(), Src::Shop(t));
        let ops = w.take_ops();
        assert!(ops.iter().any(|o| matches!(o, Op::Tell { legacy, .. } if legacy.ends_with("Empty shop"))), "{ops:?}");
    }

    #[test]
    fn page_buttons_sit_in_the_shop_types_cells() {
        let mut w = world();
        player(&mut w, 0, vec![]);
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_mut(&t).unwrap().page_mut(1);
        w.shop_display(0xA1, t.clone(), Src::Shop(t));
        let (_, _, items) = opened(&mut w);
        assert_eq!(items[41].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("Next page (1)"));
        assert!(items[39].is_none(), "no previous button on the first page");
        w.menu_click(0xA1, 5, 41, 0, false);
        let ops = w.take_ops();
        let Some(Op::SetMenuItems { items, .. }) = ops.into_iter().find(|o| matches!(o, Op::SetMenuItems { .. })) else { panic!("refresh") };
        assert_eq!(items[39].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("Previous page (1)"));
    }

    #[test]
    fn the_settings_editor_has_the_originals_buttons() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t, None, admin());
        let (title, rows, items) = opened(&mut w);
        assert_eq!((title.as_str(), rows), ("NPC Shop Editor", 2));
        let name = |i: usize| items[i].as_ref().and_then(|s| s.meta.display_name.clone());
        assert_eq!(name(0).as_deref(), Some("&fEdit permission required to view shop"));
        assert_eq!(name(2).as_deref(), Some("&fEdit shop items"));
        assert_eq!(name(4).as_deref(), Some("&fEdit shop title"));
        assert_eq!(name(8).as_deref(), Some("&fSet shop type"));
        assert_eq!(name(10).as_deref(), Some("&fView inventory"));
        assert_eq!(items[0].as_ref().unwrap().meta.lore, vec!["null".to_owned()]);
    }

    #[test]
    fn chat_sets_the_shop_title_and_permission() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t.clone(), None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 4, 0, false);
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::CloseMenu { player: 0xA1 })));
        w.menu_closed(0xA1, 5);
        assert!(w.shop_chat(0xA1, "Big Bank"));
        assert_eq!(w.shop_ref(&t).unwrap().title(), "Big Bank");
        let ops = w.take_ops();
        assert!(ops.iter().any(|o| matches!(o, Op::OpenMenu { title, .. } if title == "NPC Shop Editor")));
        assert!(!w.shop_chat(0xA1, "hello"), "the prompt is spent");
        w.menu_opened(0xA1, Some(6));
        w.menu_click(0xA1, 6, 0, 0, false);
        w.take_ops();
        w.shop_chat(0xA1, "bank.view");
        assert_eq!(w.shop_ref(&t).unwrap().view_permission.as_deref(), Some("bank.view"));
        w.menu_opened(0xA1, Some(7));
        w.menu_click(0xA1, 7, 0, 0, false);
        w.take_ops();
        w.shop_chat(0xA1, "null");
        assert_eq!(w.shop_ref(&t).unwrap().view_permission, None);
    }

    #[test]
    fn the_type_picker_lists_six_and_sets_the_chosen() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t.clone(), None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 8, 0, false);
        let (title, rows, items) = opened_next(&mut w, 6);
        assert_eq!((title.as_str(), rows), ("Set shop type", 2));
        assert_eq!(items[0].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("&aDefault"));
        assert_eq!(items[2].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("&cChest_4x9"));
        assert_eq!(items[10].as_ref().map(|s| s.id.as_str()), Some("minecraft:emerald"));
        w.menu_click(0xA1, 6, 6, 0, false);
        assert_eq!(w.shop_ref(&t).unwrap().shop_type, ShopType::Chest2x9);
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::OpenMenu { title, .. } if title == "NPC Shop Editor")), "back to the settings");
    }

    fn opened_next(w: &mut World, window: i32) -> (String, u8, Vec<Option<StackData>>) {
        let ops = w.take_ops();
        let Some(Op::OpenMenu { title, rows, items, .. }) = ops.into_iter().find(|o| matches!(o, Op::OpenMenu { .. })) else { panic!("no menu opened") };
        w.menu_opened(0xA1, Some(window));
        (title, rows, items)
    }

    #[test]
    fn the_type_picker_needs_its_node() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        let mut low = admin();
        low.permission = 0;
        w.shop_open_editor(0xA1, t, None, low);
        opened(&mut w);
        w.menu_click(0xA1, 5, 8, 0, false);
        let ops = w.take_ops();
        assert!(ops.iter().any(|o| matches!(o, Op::Tell { legacy, .. } if legacy.ends_with("You don't have permission to execute that command."))));
        assert!(!ops.iter().any(|o| matches!(o, Op::OpenMenu { .. })));
    }

    #[test]
    fn the_contents_editor_adds_an_item_through_the_item_editor() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t.clone(), None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 2, 0, false);
        let (title, rows, items) = opened_next(&mut w, 6);
        assert_eq!((title.as_str(), rows), ("NPC Shop Contents Editor (1/2)", 5));
        assert_eq!(items[41].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("New page"));
        assert_eq!(items[40].as_ref().and_then(|s| s.meta.display_name.clone()).as_deref(), Some("Edit page"));
        // An empty slot opens the item editor with no display item.
        w.menu_click(0xA1, 6, 10, 0, false);
        let (title, rows, _) = opened_next(&mut w, 7);
        assert_eq!((title.as_str(), rows), ("NPC Shop Item Editor", 6));
        // Clicking one of the player's own stacks sets the display item.
        assert_eq!(w.menu_click(0xA1, 7, 54 + 27, 0, false), MenuAction::Read { inv_slot: 0 });
        w.menu_read(0xA1, StackData::new("minecraft:emerald", 3));
        w.take_ops();
        // Save: the item is in the shop at slot 10.
        w.menu_click(0xA1, 7, 50, 0, false);
        let item = w.shop_ref(&t).unwrap().pages[0].items.get(&10).cloned().expect("saved");
        assert_eq!(item.display.as_ref().map(|d| (d.id.as_str(), d.count)), Some(("minecraft:emerald", 3)));
        assert_eq!((item.times_purchasable, item.max_repeats_on_shift_click), (0, false));
    }

    #[test]
    fn cancel_leaves_a_new_item_out_and_remove_takes_an_old_one_away() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t.clone(), None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 2, 0, false);
        opened_next(&mut w, 6);
        w.menu_click(0xA1, 6, 10, 0, false);
        opened_next(&mut w, 7);
        w.menu_read(0xA1, StackData::new("minecraft:emerald", 1));
        w.take_ops();
        w.menu_click(0xA1, 7, 48, 0, false);
        assert!(w.shop_ref(&t).unwrap().pages[0].items.get(&10).is_none(), "cancel: the new item was never saved");
        opened_next(&mut w, 8);
        // The old item at slot 4: edit then remove.
        w.menu_click(0xA1, 8, 4, 0, false);
        opened_next(&mut w, 9);
        w.menu_click(0xA1, 9, 49, 0, false);
        assert!(w.shop_ref(&t).unwrap().pages[0].items.get(&4).is_none());
    }

    #[test]
    fn the_item_editor_lays_out_cost_and_result_buttons() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t, None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 2, 0, false);
        opened_next(&mut w, 6);
        w.menu_click(0xA1, 6, 4, 0, false);
        let (_, _, items) = opened_next(&mut w, 7);
        let name = |i: usize| items[i].as_ref().and_then(|s| s.meta.display_name.clone());
        // Money is left out (no economy), so XP Level is the fourth button: row 1, column 0.
        assert_eq!(name(0).as_deref(), Some("Item Cost"));
        assert_eq!(name(1).as_deref(), Some("Permission Cost"));
        assert_eq!(name(2).as_deref(), Some("Command Cost"));
        assert_eq!(name(9).as_deref(), Some("XP Level Cost"));
        assert_eq!(items[9].as_ref().unwrap().meta.lore, vec!["3 levels".to_owned()]);
        assert_eq!(name(6).as_deref(), Some("Item Result"));
        assert_eq!(name(15).as_deref(), Some("XP Level Result"));
        assert_eq!(items[6].as_ref().unwrap().meta.lore, vec!["2 Apple".to_owned()]);
        assert_eq!(name(33).as_deref(), Some("Number of purchases limit per player"));
        assert_eq!(items[33].as_ref().unwrap().meta.lore, vec!["Times purchasable: 0 (no limit)".to_owned()]);
        assert_eq!(name(50).as_deref(), Some("&aSave"));
    }

    #[test]
    fn the_experience_cost_is_set_by_chat_and_a_bad_number_asks_again() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t.clone(), None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 2, 0, false);
        opened_next(&mut w, 6);
        w.menu_click(0xA1, 6, 4, 0, false);
        opened_next(&mut w, 7);
        w.menu_click(0xA1, 7, 9, 0, false);
        w.take_ops();
        w.menu_closed(0xA1, 7);
        assert!(w.shop_chat(0xA1, "abc"));
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Tell { .. })));
        assert!(w.shop_chat(0xA1, "7"));
        let (_, _, items) = opened_next(&mut w, 8);
        assert_eq!(items[9].as_ref().unwrap().meta.lore, vec!["7 levels".to_owned()]);
        w.menu_click(0xA1, 8, 50, 0, false);
        let it = &w.shop_ref(&t).unwrap().pages[0].items[&4];
        assert_eq!(it.cost, vec![Action::Experience { exp: 7 }]);
    }

    #[test]
    fn the_command_editor_collects_commands() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_open_editor(0xA1, t.clone(), None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 2, 0, false);
        opened_next(&mut w, 6);
        w.menu_click(0xA1, 6, 4, 0, false);
        opened_next(&mut w, 7);
        w.menu_click(0xA1, 7, 2, 0, false);
        let (title, rows, _) = opened_next(&mut w, 8);
        assert_eq!((title.as_str(), rows), ("Command editor", 4));
        w.menu_click(0xA1, 8, 0, 0, false);
        w.take_ops();
        w.menu_closed(0xA1, 8);
        assert!(w.shop_chat(0xA1, "give <player> stone"));
        let (_, _, items) = opened_next(&mut w, 9);
        let c = items[0].as_ref().unwrap();
        assert_eq!(c.meta.display_name.as_deref(), Some("&fSet command"));
        assert_eq!(c.meta.lore, vec!["Right click to remove".to_owned(), "Currently: give <player> stone".to_owned()]);
        assert_eq!(items[30].as_ref().unwrap().meta.lore, vec!["&cOff".to_owned()]);
        w.menu_click(0xA1, 9, 30, 0, false);
        // Back (the screen's close callback): the command is in the result list.
        w.menu_closed(0xA1, 9);
        let ops = w.take_ops();
        assert!(!ops.iter().any(|o| matches!(o, Op::OpenMenu { .. })));
    }

    #[test]
    fn storage_viewer_takes_and_deposits_and_saves_on_close() {
        let mut w = world();
        let t = shop_with_apple_for_xp(&mut w);
        w.shop_mut(&t).unwrap().storage.inventory = vec![StackData::new("minecraft:stone", 5)];
        w.shop_open_editor(0xA1, t.clone(), None, admin());
        opened(&mut w);
        w.menu_click(0xA1, 5, 10, 0, false);
        let (title, rows, items) = opened_next(&mut w, 6);
        assert_eq!((title.as_str(), rows), ("Item storage", 4));
        assert_eq!(items[0].as_ref().map(|s| s.count), Some(5));
        assert_eq!(items[28].as_ref().unwrap().meta.lore, vec!["&aOn".to_owned()]);
        assert_eq!(w.menu_click(0xA1, 6, 36 + 3, 0, false), MenuAction::Deposit { inv_slot: 12 });
        assert_eq!(w.shop_deposit(0xA1, StackData::new("minecraft:apple", 2)), Some(true));
        let MenuAction::Take(st) = w.menu_click(0xA1, 6, 0, 0, false) else { panic!("take") };
        assert_eq!(st.id, "minecraft:stone");
        w.menu_closed(0xA1, 6);
        let inv = &w.shop_ref(&t).unwrap().storage.inventory;
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].id, "minecraft:apple");
    }

    #[test]
    fn the_viewer_runs_commands_permissions_and_open_shop_through_ops() {
        let mut w = world();
        player(&mut w, 0, vec![]);
        let t = shop_with_apple_for_xp(&mut w);
        let mut other = Shop::new("vault");
        other.page_mut(0);
        w.shops.global.insert("vault".into(), other);
        {
            let it = w.shop_mut(&t).unwrap().pages[0].items.get_mut(&4).unwrap();
            it.cost.clear();
            it.result = vec![
                Action::Command { commands: vec!["say hi <player>".into()], op: false, server: true },
                Action::Permission { permissions: vec!["kit.vip".into()] },
            ];
        }
        w.shop_display(0xA1, t.clone(), Src::Shop(t));
        opened(&mut w);
        w.menu_click(0xA1, 5, 4, 0, false);
        let ops = w.take_ops();
        let consoles: Vec<String> = ops.iter().filter_map(|o| if let Op::Console(c) = o { Some(c.clone()) } else { None }).collect();
        assert_eq!(consoles[0], "say hi Alex");
        assert_eq!(consoles[1], "lp user 00000000-0000-0000-0000-0000000000a1 permission set kit.vip true");
    }

    #[test]
    fn shop_find_resolves_npc_ids_and_prefers_npc_shops() {
        let mut w = world();
        shop_with_apple_for_xp(&mut w);
        assert_eq!(w.shop_find("bank"), Some(Target::Global("bank".into())));
        assert_eq!(w.shop_find("nothing"), None);
        w.shops.npc.insert("bank".into(), Shop::new("bank"));
        assert_eq!(w.shop_find("bank"), Some(Target::Npc("bank".into())));
    }
}
