//! The mob-specific and the remaining `/npc` modifiers: `allay`, `armorstand`, `wolf`, `villager`,
//! `display`, `home`, `pathopt`, `playerfilter`, `potioneffect`, `attribute`, `speak`, `panimate`,
//! `undo` and about sixty more.
//!
//! Written from the public Citizens wiki (the command table with each modifier's arguments and
//! one-line description, and the `plugin.yml` node pattern `citizens.npc.<modifier>`) only;
//! Citizens' source was not read (it is under the Open Software License 3.0). What the wiki does
//! not say is decided here and listed in the README ("Docs gaps"):
//! - the reply texts and the save keys (`traits.<modifier>.<key>`) are this port's own;
//! - a flag whose meaning the wiki does not give (`horse -t`, `axolotl -d`) is saved under its
//!   letter;
//! - option values are checked only where the wiki or the game fixes them (numbers, booleans,
//!   `x,y,z`); a name (`--variant`, `--type`) is any word, saved in upper case;
//! - settings are saved; the ones listed in [`live_nbt`] also change the summoned entity, and the
//!   reply of every other one says that Horizon does not act on it yet.
//!
//! A table row ([`Mob`]) carries everything the command needs: the usage line, the mob types it
//! works on, its `--options`, its `-f` flags and its positional words. The same row feeds the
//! permission nodes, the help, the tab completion and the saved values.

use super::*;
use crate::horizon_citizens_core::look;
use crate::horizon_citizens_core::world::Op;

const IMAX: i64 = 2_147_483_647;
const NUM: (f64, f64) = (-1.0e9, 1.0e9);
const NUM0: (f64, f64) = (0.0, 1.0e9);

#[derive(Clone, Copy)]
enum V {
    Bool,
    Int(i64, i64),
    Num(f64, f64),
    /// One of these words (any case, `-`/space as `_`); saved in upper case.
    Words(&'static [&'static str]),
    Any,
    /// Every word up to the next `--option`.
    Text,
    /// `x,y,z`.
    Vec3,
    /// `x,y,z,w`.
    Vec4,
    /// `a,b`, each a whole number from 0 up to the limit.
    Pair(i64),
    /// `x,y,z` or `x,y,z,world`.
    Loc,
}

struct Opt {
    key: &'static str,
    v: V,
}

const fn o(key: &'static str, v: V) -> Opt {
    Opt { key, v }
}

/// `-<ch>` sets `key` to `val` (and `key2` too when it is not empty).
struct Flag {
    ch: char,
    key: &'static str,
    val: bool,
    key2: &'static str,
}

const fn f(ch: char, key: &'static str, val: bool) -> Flag {
    Flag { ch, key, val, key2: "" }
}

pub(super) struct Mob {
    cmd: Cmd,
    /// The mob types it works on (Bukkit names; `*SUFFIX` matches a suffix); empty: any.
    kinds: &'static [&'static str],
    opts: &'static [Opt],
    flags: &'static [Flag],
    /// Positional words in order, the last of them may be text.
    pos: &'static [Opt],
    /// How many of `pos` must be given.
    req: usize,
    /// With nothing to set, flip this boolean instead of describing the settings.
    toggle: &'static str,
    /// Whether the saved settings change the summoned entity ([`live_nbt`]).
    live: bool,
}

macro_rules! m {
    ($name:literal, [$($alias:literal),*], $usage:literal) => {
        Mob {
            cmd: cmd($name, &[$($alias),*], $usage, 1, None, concat!("citizens.npc.", $name)),
            kinds: &[],
            opts: &[],
            flags: &[],
            pos: &[],
            req: 0,
            toggle: "",
            live: false,
        }
    };
}

const MOBS: &[Mob] = &[
    Mob { kinds: &["ALLAY"], flags: &[f('d', "dancing", true)], ..m!("allay", [], "allay (-d(ancing))") },
    Mob {
        kinds: &["AREA_EFFECT_CLOUD"],
        opts: &[o("color", V::Any), o("duration", V::Int(0, IMAX)), o("radius", V::Num(NUM0.0, NUM0.1)), o("radius_per_tick", V::Num(NUM.0, NUM.1)), o("particle", V::Any)],
        ..m!("areaeffectcloud", [], "areaeffectcloud --color [color] --duration [duration] --radius [radius] --radius_per_tick [radius] --particle [particle]")
    },
    Mob { kinds: &["ARMADILLO"], opts: &[o("state", V::Words(&["idle", "rolling", "scared", "unrolling"]))], ..m!("armadillo", [], "armadillo --state [state]") },
    Mob {
        kinds: &["ARMOR_STAND"],
        opts: &[
            o("visible", V::Bool),
            o("small", V::Bool),
            o("marker", V::Bool),
            o("gravity", V::Bool),
            o("arms", V::Bool),
            o("baseplate", V::Bool),
            o("headpose", V::Vec3),
            o("bodypose", V::Vec3),
            o("leftarmpose", V::Vec3),
            o("leftlegpose", V::Vec3),
            o("rightarmpose", V::Vec3),
            o("rightlegpose", V::Vec3),
        ],
        live: true,
        ..m!("armorstand", [], "armorstand --visible [visible] --small [small] --marker [marker] --gravity [gravity] --arms [arms] --baseplate [baseplate] --(head|body|leftarm|leftleg|rightarm|rightleg)pose [angle x,y,z]")
    },
    Mob {
        kinds: &["AXOLOTL"],
        flags: &[f('d', "d", true)],
        opts: &[o("variant", V::Words(&["lucy", "wild", "gold", "cyan", "blue"]))],
        ..m!("axolotl", [], "axolotl (-d) (--variant variant)")
    },
    Mob { kinds: &["BAT"], opts: &[o("awake", V::Bool)], ..m!("bat", [], "bat --awake [awake]") },
    Mob {
        kinds: &["BEE"],
        flags: &[f('s', "stung", true), f('n', "stung", false)],
        opts: &[o("anger", V::Int(0, IMAX))],
        ..m!("bee", [], "bee (-s/-n) --anger anger")
    },
    Mob { kinds: &["*BOAT", "*RAFT"], opts: &[o("type", V::Any)], ..m!("boat", [], "boat --type [type]") },
    Mob {
        opts: &[
            o("style", V::Words(&["solid", "segmented_6", "segmented_10", "segmented_12", "segmented_20"])),
            o("color", V::Words(&["pink", "blue", "red", "green", "yellow", "purple", "white"])),
            o("title", V::Text),
            o("visible", V::Bool),
            o("viewpermission", V::Any),
            o("flags", V::Any),
            o("track", V::Any),
            o("range", V::Num(NUM0.0, NUM0.1)),
        ],
        ..m!("bossbar", [], "bossbar --style [style] --color [color] --title [title] --visible [visible] --viewpermission [permission] --flags [flags] --track [health | placeholder] --range [range]")
    },
    Mob { opts: &[o("location", V::Vec3), o("radius", V::Num(NUM0.0, NUM0.1))], ..m!("breakblock", [], "breakblock --location [x,y,z] --radius [radius]") },
    Mob { kinds: &["CAMEL"], opts: &[o("pose", V::Any)], ..m!("camel", [], "camel (--pose pose)") },
    Mob {
        kinds: &["CAT"],
        flags: &[f('s', "sitting", true), f('n', "sitting", false), f('l', "lying", true)],
        opts: &[o("type", V::Any), o("ccolor", V::Any)],
        ..m!("cat", [], "cat (-s/-n/-l) --type type --ccolor collar color")
    },
    Mob { kinds: &["CHICKEN"], opts: &[o("variant", V::Any)], ..m!("chicken", [], "chicken (--variant variant)") },
    Mob {
        flags: &[f('m', "mount", true), f('o', "owner_required", true)],
        opts: &[o("controls", V::Any), o("enabled", V::Bool)],
        toggle: "enabled",
        ..m!("controllable", ["control"], "controllable (-m(ount),-o(wner required)) (--controls [controls]) (--enabled [true|false])")
    },
    Mob {
        kinds: &["COPPER_GOLEM"],
        opts: &[o("weatherstate", V::Any), o("weatheringtick", V::Int(0, IMAX))],
        ..m!("coppergolem", [], "coppergolem (--weatherstate state) (--weatheringtick tick)")
    },
    Mob { kinds: &["COW"], opts: &[o("variant", V::Any)], ..m!("cow", [], "cow (--variant variant)") },
    Mob {
        kinds: &["BLOCK_DISPLAY", "ITEM_DISPLAY", "TEXT_DISPLAY"],
        opts: &[
            o("billboard", V::Words(&["fixed", "vertical", "horizontal", "center"])),
            o("brightness", V::Pair(15)),
            o("interpolationdelay", V::Int(-IMAX, IMAX)),
            o("interpolationduration", V::Int(0, IMAX)),
            o("height", V::Num(NUM0.0, NUM0.1)),
            o("width", V::Num(NUM0.0, NUM0.1)),
            o("scale", V::Vec3),
            o("viewrange", V::Num(NUM0.0, NUM0.1)),
            o("leftrotation", V::Vec4),
            o("rightrotation", V::Vec4),
            o("offset", V::Vec3),
        ],
        ..m!("display", [], "display --billboard [billboard] --brightness [blockLight,skyLight] --interpolationdelay [delay] --interpolationduration [duration] --height [height] --width [width] --scale [x,y,z] --viewrange [range] --leftrotation [x,y,z,w] --rightrotation [x,y,z,w] --offset [x,y,z]")
    },
    Mob { kinds: &["END_CRYSTAL", "ENDER_CRYSTAL"], flags: &[f('b', "bottom", true)], ..m!("endercrystal", [], "endercrystal -b(ottom)") },
    Mob {
        kinds: &["ENDER_DRAGON"],
        opts: &[o("phase", V::Any), o("destroywalls", V::Bool)],
        ..m!("enderdragon", [], "enderdragon --phase [phase] --destroywalls [true|false]")
    },
    Mob { kinds: &["ENDERMAN"], flags: &[f('a', "angry", true)], ..m!("enderman", [], "enderman -a(ngry)") },
    Mob {
        opts: &[o("width", V::Num(NUM0.0, NUM0.1)), o("height", V::Num(NUM0.0, NUM0.1)), o("strength", V::Num(NUM.0, NUM.1)), o("vertical-strength", V::Num(NUM.0, NUM.1))],
        ..m!("forcefield", [], "forcefield --width [width] --height [height] --strength [strength] --vertical-strength [vertical strength]")
    },
    Mob {
        kinds: &["FOX"],
        opts: &[
            o("type", V::Any),
            o("sleeping", V::Bool),
            o("sitting", V::Bool),
            o("crouching", V::Bool),
            o("interested", V::Bool),
            o("pouncing", V::Bool),
            o("faceplanted", V::Bool),
        ],
        ..m!("fox", [], "fox --type type --sleeping [true|false] --sitting [true|false] --crouching [true|false] --interested [true|false] --pouncing [true|false] --faceplanted [true|false]")
    },
    Mob { kinds: &["FROG"], opts: &[o("variant", V::Any)], ..m!("frog", [], "frog (--variant variant)") },
    Mob {
        kinds: &["GOAT"],
        flags: &[
            f('l', "left_horn", true),
            f('r', "right_horn", true),
            Flag { ch: 'n', key: "left_horn", val: false, key2: "right_horn" },
            Flag { ch: 'b', key: "left_horn", val: true, key2: "right_horn" },
        ],
        pos: &[o("horn", V::Any)],
        ..m!("goat", [], "goat -l(eft) -r(ight) -n(either) -b(oth) horn")
    },
    Mob {
        opts: &[o("scale", V::Num(NUM0.0, NUM0.1)), o("width", V::Num(NUM0.0, NUM0.1)), o("height", V::Num(NUM0.0, NUM0.1)), o("offset", V::Vec3)],
        ..m!("hitbox", [], "hitbox --scale [scale] --width/height [value] --offset [x,y,z]")
    },
    Mob {
        flags: &[f('h', "here", true), f('p', "pathfind", true), f('t', "teleport", true)],
        opts: &[o("location", V::Loc), o("delay", V::Int(0, IMAX)), o("distance", V::Num(NUM0.0, NUM0.1))],
        ..m!("home", [], "home --location [loc] --delay [delay] --distance [distance] -h(ere) -p(athfind) -t(eleport)")
    },
    Mob {
        kinds: &["HORSE", "DONKEY", "MULE", "SKELETON_HORSE", "ZOMBIE_HORSE"],
        flags: &[f('c', "chest", true), f('b', "chest", false), f('t', "t", true)],
        opts: &[o("color", V::Any), o("type", V::Any), o("style", V::Any)],
        ..m!("horse", [], "horse (--color color) (--type type) (--style style) (-cbt)")
    },
    Mob { flags: &[f('h', "hand", true)], pos: &[o("item", V::Any)], ..m!("item", [], "item [item] (-h(and))") },
    Mob { kinds: &["ITEM_DISPLAY"], opts: &[o("transform", V::Any)], ..m!("itemdisplay", [], "itemdisplay --transform [transform]") },
    Mob {
        kinds: &["ITEM_FRAME", "GLOW_ITEM_FRAME"],
        opts: &[o("visible", V::Bool), o("fixed", V::Bool), o("rotation", V::Any), o("item", V::Any), o("face", V::Any)],
        ..m!("itemframe", [], "itemframe --visible [true|false] --fixed [true|false] --rotation [rotation] --item [item] --face [face]")
    },
    Mob {
        kinds: &["LLAMA", "TRADER_LLAMA"],
        opts: &[o("color", V::Any), o("strength", V::Int(1, 5))],
        ..m!("llama", [], "llama (--color color) (--strength strength)")
    },
    Mob {
        kinds: &["MANNEQUIN"],
        opts: &[
            o("hide_description", V::Bool),
            o("immovable", V::Bool),
            o("description", V::Text),
            o("main_hand", V::Words(&["left", "right"])),
            o("skin_patch_type", V::Any),
            o("skin_patch_body", V::Any),
            o("skin_patch_cape", V::Any),
            o("skin_patch_elytra", V::Any),
        ],
        ..m!("mannequin", [], "mannequin --hide_description [true|false] --immovable [true|false] --description [description] --main_hand [LEFT|RIGHT] --skin_patch_type [type] --skin_patch_body [location] --skin_patch_cape [location] --skin_patch_elytra [location]")
    },
    Mob { kinds: &["*MINECART"], opts: &[o("offset", V::Int(-IMAX, IMAX))], ..m!("minecart", [], "minecart (--offset offset)") },
    Mob {
        kinds: &["PLAYER"],
        opts: &[o("name", V::Bool), o("equipment", V::Bool)],
        ..m!("mirror", [], "mirror --name [true|false] --equipment [true|false]")
    },
    Mob { pos: &[o("name", V::Any)], req: 1, ..m!("model", [], "model [name]") },
    Mob {
        pos: &[o("action", V::Words(&["start", "stop", "clear"])), o("name", V::Any), o("speed", V::Num(NUM.0, NUM.1))],
        req: 1,
        ..m!("modelanimate", [], "modelanimate [start|stop|clear] (name) (speed)")
    },
    Mob { kinds: &["MUSHROOM_COW", "MOOSHROOM"], opts: &[o("variant", V::Any)], ..m!("mushroomcow", [], "mushroomcow (--variant [variant])") },
    Mob {
        kinds: &["OCELOT"],
        flags: &[f('s', "sitting", true), f('n', "sitting", false)],
        opts: &[o("type", V::Any)],
        ..m!("ocelot", [], "ocelot (--type type) (-s(itting), -n(ot sitting))")
    },
    Mob { opts: &[o("enabled", V::Bool)], ..m!("packet", [], "packet --enabled [true|false]") },
    Mob { kinds: &["PAINTING"], opts: &[o("art", V::Any)], ..m!("painting", [], "painting (--art art)") },
    Mob {
        kinds: &["PANDA"],
        flags: &[f('e', "eating", true), f('s', "sitting", true), f('n', "sneezing", true), f('r', "rolling", true)],
        opts: &[o("gene", V::Any), o("hiddengene", V::Any)],
        ..m!("panda", [], "panda --gene (main gene) --hiddengene (hidden gene) -e(ating) -s(itting) -n (sneezing) -r(olling)")
    },
    Mob { kinds: &["PARROT"], opts: &[o("variant", V::Any)], ..m!("parrot", [], "parrot (--variant variant)") },
    Mob {
        opts: &[
            o("avoid-teleporting", V::Bool),
            o("avoid-water", V::Bool),
            o("attack-delay-duration", V::Int(0, IMAX)),
            o("destination-teleport-margin", V::Num(NUM0.0, NUM0.1)),
            o("open-doors", V::Bool),
            o("path-range", V::Num(NUM0.0, NUM0.1)),
            o("stationary-ticks", V::Int(0, IMAX)),
            o("attack-range", V::Num(NUM0.0, NUM0.1)),
            o("distance-margin", V::Num(NUM0.0, NUM0.1)),
            o("path-distance-margin", V::Num(NUM0.0, NUM0.1)),
            o("pathfinder-type", V::Words(&["citizens", "minecraft"])),
            o("falling-distance", V::Int(-1, IMAX)),
        ],
        ..m!("pathopt", ["po", "patho"], "pathopt --avoid-teleporting [true|false] --avoid-water [true|false] --attack-delay-duration [duration] --destination-teleport-margin [margin] --open-doors [true|false] --path-range [range] --stationary-ticks [ticks] --attack-range [range] --distance-margin [margin] --path-distance-margin [margin] --pathfinder-type [CITIZENS|MINECRAFT] --falling-distance [distance]")
    },
    Mob {
        opts: &[o("onrightclick", V::Bool), o("when-player-within", V::Num(NUM0.0, NUM0.1)), o("pauseduration", V::Int(0, IMAX)), o("lockoutduration", V::Int(0, IMAX))],
        ..m!("pausepathfinding", [], "pausepathfinding --onrightclick [true|false] --when-player-within [range in blocks] --pauseduration [duration] --lockoutduration [duration]")
    },
    Mob { kinds: &["PHANTOM"], opts: &[o("size", V::Int(0, 64))], live: true, ..m!("phantom", [], "phantom (--size size)") },
    Mob { kinds: &["PIG"], opts: &[o("variant", V::Any)], ..m!("pig", [], "pig (--variant variant)") },
    Mob { kinds: &["PIGLIN"], opts: &[o("dancing", V::Bool)], ..m!("piglin", [], "piglin (--dancing [true|false])") },
    Mob {
        flags: &[f('a', "allowlist", true), f('e', "empty", true), f('d', "denylist", true), f('c', "clear", true)],
        opts: &[
            o("add", V::Any),
            o("remove", V::Any),
            o("addpermission", V::Any),
            o("removepermission", V::Any),
            o("addgroup", V::Any),
            o("removegroup", V::Any),
            o("applywithin", V::Num(NUM0.0, NUM0.1)),
        ],
        ..m!("playerfilter", [], "playerfilter -a(llowlist) -e(mpty) -d(enylist) --add [uuid] --remove [uuid] --addpermission [permission] --removepermission [permission] --addgroup [group] --removegroup [group] -c(lear) --applywithin [blocks range]")
    },
    Mob { kinds: &["POLAR_BEAR"], flags: &[f('r', "rearing", true)], ..m!("polarbear", [], "polarbear (-r)") },
    Mob {
        flags: &[f('t', "temporary", true)],
        opts: &[o("name", V::Any), o("type", V::Any), o("duration", V::Int(-1, IMAX)), o("amplifier", V::Int(0, 255)), o("icon", V::Bool)],
        pos: &[o("action", V::Words(&["add", "remove", "list"]))],
        req: 1,
        ..m!("potioneffect", [], "potioneffect [add|remove|list] (--name [name] or -t for temporary) (--type [type]) (--duration [duration]) (--amplifier [amplifier]) (--icon [icon])")
    },
    Mob { kinds: &["PUFFERFISH"], opts: &[o("state", V::Int(0, 2))], live: true, ..m!("pufferfish", [], "pufferfish (--state state)") },
    Mob {
        kinds: &["RABBIT"],
        pos: &[o("type", V::Words(&["brown", "white", "black", "black_and_white", "gold", "salt_and_pepper", "the_killer_bunny"]))],
        req: 1,
        ..m!("rabbittype", ["rbtype"], "rabbittype [type]")
    },
    Mob {
        opts: &[
            o("link_body", V::Bool),
            o("head_only", V::Bool),
            o("max_pitch_per_tick", V::Num(NUM0.0, NUM0.1)),
            o("max_yaw_per_tick", V::Num(NUM0.0, NUM0.1)),
            o("pitch_range", V::Any),
            o("yaw_range", V::Any),
        ],
        pos: &[o("mode", V::Words(&["linear", "immediate"]))],
        ..m!("rotationsettings", [], "rotationsettings [linear|immediate] (--link_body) (--head_only) (--max_pitch_per_tick) (--max_yaw_per_tick) (--pitch_range) (--yaw_range)")
    },
    Mob {
        kinds: &["SHULKER"],
        opts: &[o("peek", V::Int(0, 100)), o("color", V::Any)],
        ..m!("shulker", [], "shulker (--peek [peek] --color [color])")
    },
    Mob { kinds: &["SNIFFER"], opts: &[o("state", V::Any)], ..m!("sniffer", [], "sniffer (--state [state])") },
    Mob {
        kinds: &["SNOWMAN", "SNOW_GOLEM"],
        flags: &[f('d', "derp", true), f('f', "form_snow", true)],
        ..m!("snowman", [], "snowman (-d[erp]) (-f[orm snow])")
    },
    Mob { kinds: &["EVOKER", "ILLUSIONER"], opts: &[o("spell", V::Any)], ..m!("spellcaster", [], "spellcaster (--spell spell)") },
    Mob {
        kinds: &["TEXT_DISPLAY"],
        opts: &[o("shadowed", V::Bool), o("seethrough", V::Bool), o("line_width", V::Int(0, IMAX)), o("text", V::Text)],
        live: true,
        ..m!("textdisplay", [], "textdisplay --shadowed [true|false] --seethrough [true|false] --line_width [width] --text [text]")
    },
    Mob {
        kinds: &["TROPICAL_FISH"],
        opts: &[o("body", V::Any), o("pattern", V::Any), o("patterncolor", V::Any)],
        ..m!("tropicalfish", [], "tropicalfish (--body color) (--pattern pattern) (--patterncolor color)")
    },
    Mob { flags: &[f('o', "offhand", true)], ..m!("useitem", [], "useitem (-o(ffhand))") },
    Mob { kinds: &["VEX"], opts: &[o("charging", V::Bool)], ..m!("vex", [], "vex (--charging [charging])") },
    Mob {
        kinds: &["VILLAGER", "ZOMBIE_VILLAGER"],
        flags: &[f('s', "shake_head", true)],
        opts: &[o("level", V::Int(1, 5)), o("type", V::Any), o("profession", V::Any)],
        live: true,
        ..m!("villager", [], "villager (--level level) (--type type) (--profession profession) -s(hake head)")
    },
    Mob {
        kinds: &["WITHER"],
        opts: &[o("invulnerable", V::Bool), o("invulnerable-ticks", V::Int(0, IMAX)), o("arrow-shield", V::Bool)],
        ..m!("wither", [], "wither (--invulnerable [true|false]) (--invulnerable-ticks [ticks]) (--arrow-shield [true|false])")
    },
    Mob {
        kinds: &["WOLF"],
        flags: &[f('s', "sitting", true), f('a', "angry", true), f('t', "tamed", true), f('i', "interested", true)],
        opts: &[o("collar", V::Any), o("variant", V::Any)],
        ..m!("wolf", [], "wolf (-s(itting) a(ngry) t(amed) i(nterested)) --collar [hex rgb color|name] --variant [variant]")
    },
    // The ones with their own handler: they read the line themselves.
    Mob { pos: &[o("attribute", V::Any), o("value", V::Num(NUM.0, NUM.1))], ..m!("attribute", [], "attribute [attribute] [value]") },
    Mob {
        opts: &[o("bubble", V::Int(0, IMAX)), o("target", V::Any), o("range", V::Num(NUM0.0, NUM0.1))],
        pos: &[o("message", V::Text)],
        req: 1,
        ..m!("speak", [], "speak [message] --bubble [duration] --target [npcid|player name] --range (range to look for entities to speak to in blocks)")
    },
    Mob { kinds: &["PLAYER"], pos: &[o("animation", V::Any)], req: 1, ..m!("panimate", [], "panimate [animation]") },
    Mob { pos: &[o("all", V::Words(&["all"]))], ..m!("undo", [], "undo (all)") },
];

/// The commands of this file, for the table of sub-commands.
pub(super) fn cmds() -> impl Iterator<Item = &'static Cmd> {
    MOBS.iter().map(|m| &m.cmd)
}

/// The sub-command's row.
fn row(name: &str) -> Option<&'static Mob> {
    MOBS.iter().find(|m| m.cmd.name == name)
}

pub fn handles(name: &str) -> bool {
    row(name).is_some()
}

/// Every permission node of these commands.
pub fn nodes() -> Vec<&'static str> {
    MOBS.iter().map(|m| m.cmd.node).collect()
}

/// The tab-completion tails of a row: `--key <value:kind>` for every option, `-x` for every flag.
fn tails(m: &Mob) -> Vec<String> {
    let mut out = vec![String::new()];
    for fl in m.flags {
        out.push(format!("-{}", fl.ch));
    }
    for op in m.opts {
        match op.v {
            V::Bool => {
                out.push(format!("--{} true", op.key));
                out.push(format!("--{} false", op.key));
            }
            V::Words(words) => {
                for w in words {
                    out.push(format!("--{} {w}", op.key));
                }
            }
            V::Int(..) => out.push(format!("--{} <{}:integer>", op.key, kind_word(op.key))),
            V::Num(..) => out.push(format!("--{} <{}:number>", op.key, kind_word(op.key))),
            V::Text => out.push(format!("--{} <{}:text>", op.key, kind_word(op.key))),
            _ => out.push(format!("--{} <{}:word>", op.key, kind_word(op.key))),
        }
    }
    for (i, p) in m.pos.iter().enumerate() {
        if i > 0 {
            break;
        }
        match p.v {
            V::Words(words) => {
                for w in words {
                    out.push((*w).to_owned());
                }
            }
            V::Text => out.push(format!("<{}:text>", kind_word(p.key))),
            V::Num(..) => out.push(format!("<{}:number>", kind_word(p.key))),
            V::Int(..) => out.push(format!("<{}:integer>", kind_word(p.key))),
            _ => out.push(format!("<{}:word>", kind_word(p.key))),
        }
    }
    match m.cmd.name {
        "potioneffect" => {
            out.push("add --type <type:word>".into());
            out.push("add --type <type:word> --duration <duration:integer> --amplifier <amplifier:integer>".into());
            out.push("remove --name <name:word>".into());
            out.push("remove --type <type:word>".into());
        }
        "attribute" => out.push("<attribute:word> <value:number>".into()),
        "modelanimate" => out.push("start <name:word>".into()),
        _ => {}
    }
    out.sort();
    out.dedup();
    out
}

/// The usages for the game's tab completion, for the name and every alias.
pub fn usages() -> Vec<String> {
    let mut out = Vec::new();
    for m in MOBS {
        for n in std::iter::once(m.cmd.name).chain(m.cmd.aliases.iter().copied()) {
            for tail in tails(m) {
                out.push(format!("npc {n} {tail}").trim().to_owned());
            }
        }
    }
    out
}

/// A tab-completion argument name: letters only.
fn kind_word(key: &str) -> String {
    key.chars().filter(char::is_ascii_alphabetic).collect()
}

const M_SET: &str = "[[{0}]]''s {1} set to [[{2}]].";
const M_DESCRIBE: &str = "[[{0}]]''s {1} settings: {2}.";
const M_TOGGLED: &str = "[[{0}]]''s {1} is now [[{2}]].";
const M_BAD_VALUE: &str = "[[{0}]] is not a valid value for [[{1}]].";
const M_BAD_CHOICE: &str = "Valid values are: {0}.";
const M_RANGE: &str = "The value of [[{0}]] must be between [[{1}]] and [[{2}]].";
const M_NOT_ENFORCED: &str = "This setting is saved, but Horizon does not act on it yet.";
const M_UNKNOWN_NAME: &str = "[[{0}]] is not a valid [[{1}]].";
const M_NO_ENTRY: &str = "No [[{0}]] named [[{1}]] is set.";
const M_LISTED: &str = "[[{0}]] has {1}.";
const M_NO_TARGET: &str = "Nobody heard it: no player is within [[{0}]] blocks.";
const M_UNDONE: &str = "Undid the last action ({0}).";
const M_UNDO_NOTHING: &str = "There is nothing to undo.";
const M_PANIMATE_UNSUPPORTED: &str = "The animation [[{0}]] is not supported on Horizon yet.";
const M_PANIMATE_INVALID: &str = "Valid animations are: {0}.";

/// The animations `/npc panimate` names (a player's `PlayerAnimationType` and the sneak/sleep/sit ones).
const ANIMATIONS: &[&str] = &["arm_swing", "arm_swing_offhand", "hurt", "crit", "magic_crit", "sit", "sleep", "sneak", "stop_sitting", "stop_sleeping", "stop_sneaking"];
/// How far `/npc speak` reaches without `--range`, in blocks.
const SPEAK_RANGE: f64 = 5.0;

struct Parsed {
    opts: Vec<(&'static str, V, String)>,
    flags: Vec<&'static Flag>,
    words: Vec<String>,
}

/// `Err` for an option or flag the row does not have, or an option with no value.
fn parse(m: &'static Mob, tail: &str) -> Result<Parsed, ()> {
    let tokens: Vec<&str> = tail.split_whitespace().collect();
    let mut p = Parsed { opts: Vec::new(), flags: Vec::new(), words: Vec::new() };
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if let Some(name) = t.strip_prefix("--").filter(|n| !n.is_empty()) {
            let (key, inline) = match name.split_once('=') {
                Some((k, v)) => (k.to_ascii_lowercase(), Some(v.to_owned())),
                None => (name.to_ascii_lowercase(), None),
            };
            let Some(op) = m.opts.iter().find(|op| op.key == key) else {
                return Err(());
            };
            let value = if let Some(v) = inline {
                v
            } else if matches!(op.v, V::Text) {
                let mut parts = Vec::new();
                while let Some(next) = tokens.get(i + 1) {
                    if next.starts_with("--") {
                        break;
                    }
                    parts.push(*next);
                    i += 1;
                }
                parts.join(" ")
            } else if matches!(op.v, V::Bool) {
                // The usage shows `--small [small]`: the next plain word is the value, checked by `check` (so
                // `maybe` answers "not a valid value"); a flag or nothing after it means true.
                match tokens.get(i + 1) {
                    Some(v) if !v.starts_with('-') => {
                        i += 1;
                        v.to_ascii_lowercase()
                    }
                    _ => "true".to_owned(),
                }
            } else {
                match tokens.get(i + 1) {
                    Some(v) => {
                        i += 1;
                        (*v).to_owned()
                    }
                    None => return Err(()),
                }
            };
            p.opts.push((op.key, op.v, value));
        } else if let Some(sw) = t.strip_prefix('-').filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic())) {
            for c in sw.chars() {
                let c = c.to_ascii_lowercase();
                let Some(fl) = m.flags.iter().find(|fl| fl.ch == c) else {
                    return Err(());
                };
                p.flags.push(fl);
            }
        } else {
            p.words.push(t.to_owned());
        }
        i += 1;
    }
    Ok(p)
}

/// A value as the node to save and the text to show; `Err` carries the message.
fn check(w: &World, key: &str, v: V, raw: &str) -> Result<(Node, String), Reply> {
    let bad = || err(w, M_BAD_VALUE, &[raw, key]);
    match v {
        V::Bool => match raw.to_ascii_lowercase().as_str() {
            "true" => Ok((Node::bool(true), "true".into())),
            "false" => Ok((Node::bool(false), "false".into())),
            _ => Err(bad()),
        },
        V::Int(lo, hi) => {
            let n: i64 = raw.parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
            if n < lo || n > hi {
                return Err(err(w, M_RANGE, &[key, &lo.to_string(), &hi.to_string()]));
            }
            Ok((Node::int(n), n.to_string()))
        }
        V::Num(lo, hi) => {
            let n: f64 = raw.parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
            if !n.is_finite() {
                return Err(err(w, M_INVALID_NUMBER, &[]));
            }
            if n < lo || n > hi {
                return Err(err(w, M_RANGE, &[key, &dec2(lo), &dec2(hi)]));
            }
            Ok((Node::float(n), dec2(n)))
        }
        V::Words(words) => {
            let word = raw.to_ascii_lowercase().replace(['-', ' '], "_");
            if words.contains(&word.as_str()) {
                let up = word.to_ascii_uppercase();
                Ok((Node::str(&up), up))
            } else {
                let mut out = err(w, M_BAD_VALUE, &[raw, key]);
                out.extend(err(w, M_BAD_CHOICE, &[&words.join(", ")]));
                Err(out)
            }
        }
        V::Any | V::Text => {
            if raw.is_empty() {
                return Err(bad());
            }
            Ok((Node::str(raw), raw.to_owned()))
        }
        V::Vec3 | V::Vec4 | V::Loc | V::Pair(_) => {
            let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
            let want = match v {
                V::Vec3 => 3,
                V::Vec4 => 4,
                V::Pair(_) => 2,
                _ => 0,
            };
            let loc_ok = matches!(v, V::Loc) && (parts.len() == 3 || parts.len() == 4);
            if !loc_ok && parts.len() != want {
                return Err(bad());
            }
            let numeric = if matches!(v, V::Loc) { &parts[..3] } else { &parts[..] };
            for part in numeric {
                match v {
                    V::Pair(hi) => {
                        if part.parse::<i64>().ok().filter(|n| (0..=hi).contains(n)).is_none() {
                            return Err(bad());
                        }
                    }
                    _ => {
                        if part.parse::<f64>().ok().filter(|n| n.is_finite()).is_none() {
                            return Err(bad());
                        }
                    }
                }
            }
            let text = parts.join(",");
            Ok((Node::str(&text), text))
        }
    }
}

fn kind_fits(m: &Mob, kind: &str) -> bool {
    m.kinds.is_empty()
        || m.kinds.iter().any(|k| match k.strip_prefix('*') {
            Some(suffix) => kind.ends_with(suffix),
            None => *k == kind,
        })
}

fn saved(w: &World, id: i32, name: &str, key: &str) -> Option<Node> {
    w.saves.get(id).and_then(|n| n.map.at(&["traits", name, key])).cloned()
}

/// Runs `/npc <name> <tail>` for one of the rows.
pub fn run(w: &mut World, sender: &Sender, name: &str, tail: &str) -> Res {
    let Some(m) = row(name) else {
        return Err(Vec::new());
    };
    // `undo` takes back a create or remove, not an edit of one NPC: it needs no selected NPC.
    if name == "undo" {
        let Ok(parsed) = parse(m, tail) else {
            return Err(usage_of(w, name));
        };
        return undo(w, sender, &parsed);
    }
    let id = target(w, sender)?;
    let kind = w.saves.get(id).map(NpcData::kind).unwrap_or_default().to_ascii_uppercase();
    if !kind_fits(m, &kind) {
        return Err(err(w, M_REQ_MOBTYPE, &[&pretty_enum(&kind)]));
    }
    let Ok(parsed) = parse(m, tail) else {
        return Err(usage_of(w, name));
    };
    match name {
        "attribute" => return attribute(w, id, &parsed),
        "playerfilter" => return playerfilter(w, id, m, &parsed),
        "potioneffect" => return potioneffect(w, id, &parsed),
        "speak" => return speak(w, sender, id, tail),
        "panimate" => return panimate(w, id, &parsed),
        "undo" => return undo(w, sender, &parsed),
        _ => {}
    }
    generic(w, id, m, parsed)
}

fn generic(w: &mut World, id: i32, m: &'static Mob, parsed: Parsed) -> Res {
    let name = m.cmd.name;
    let npc_name = name_at(w, id);
    // Positional words: the last one may be text and take the rest.
    let mut sets: Vec<(&str, Node, String)> = Vec::new();
    if parsed.words.len() > m.pos.len() && !m.pos.last().is_some_and(|p| matches!(p.v, V::Text)) {
        return Err(usage_of(w, name));
    }
    for (i, p) in m.pos.iter().enumerate() {
        let raw = if matches!(p.v, V::Text) && i == m.pos.len() - 1 {
            parsed.words.get(i..).map(|r| r.join(" ")).filter(|s| !s.is_empty())
        } else {
            parsed.words.get(i).cloned()
        };
        if let Some(raw) = raw {
            let (node, shown) = check(w, p.key, p.v, &raw)?;
            sets.push((p.key, node, shown));
        }
    }
    if parsed.words.len() < m.req {
        return Err(usage_of(w, name));
    }
    for (key, v, raw) in &parsed.opts {
        let (node, shown) = check(w, key, *v, raw)?;
        sets.push((key, node, shown));
    }
    for fl in &parsed.flags {
        sets.push((fl.key, Node::bool(fl.val), fl.val.to_string()));
        if !fl.key2.is_empty() {
            sets.push((fl.key2, Node::bool(fl.val), fl.val.to_string()));
        }
    }
    if sets.is_empty() {
        if !m.toggle.is_empty() {
            let now = !saved(w, id, name, m.toggle).and_then(|n| n.as_bool()).unwrap_or(false);
            {
                let npc = npc_mut(w, id)?;
                npc.map.put(&["traits", name, m.toggle], Node::bool(now));
                npc.add_trait_name(name);
            }
            let out = ok(w, M_TOGGLED, &[&npc_name, name, if now { "enabled" } else { "disabled" }]);
            return Ok(finish(w, id, m.live, out));
        }
        let have: Vec<String> = w
            .saves
            .get(id)
            .and_then(|n| n.map.at(&["traits", name]))
            .map(|n| n.entries().iter().map(|e| format!("{} {}", e.key, node_text(&e.value))).collect())
            .unwrap_or_default();
        let shown = if have.is_empty() { "none set".to_owned() } else { have.join(", ") };
        return Ok(ok(w, M_DESCRIBE, &[&npc_name, name, &shown]));
    }
    let mut out = Vec::new();
    {
        let npc = npc_mut(w, id)?;
        for (key, node, _) in &sets {
            npc.map.put(&["traits", name, key], node.clone());
        }
        npc.add_trait_name(name);
    }
    for (key, _, shown) in &sets {
        out.extend(ok(w, M_SET, &[&npc_name, &format!("{name} {key}"), shown]));
    }
    Ok(finish(w, id, m.live, out))
}

fn node_text(n: &Node) -> String {
    if let Some(b) = n.as_bool() {
        b.to_string()
    } else if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(f) = n.as_f64() {
        dec2(f)
    } else {
        n.text().unwrap_or_default()
    }
}

fn finish(w: &mut World, id: i32, live: bool, mut out: Reply) -> Reply {
    w.mark_dirty();
    if live {
        w.respawn(id);
    } else {
        out.extend(ok(w, M_NOT_ENFORCED, &[]));
    }
    out
}

// ---- the rows with a handler of their own -----------------------------------------------------

/// `/npc attribute [attribute] [value]`: no argument lists the saved ones, one shows it.
fn attribute(w: &mut World, id: i32, p: &Parsed) -> Res {
    let npc_name = name_at(w, id);
    let Some(raw) = p.words.first() else {
        let have: Vec<String> = w
            .saves
            .get(id)
            .and_then(|n| n.map.at(&["traits", "attribute"]))
            .map(|n| n.entries().iter().map(|e| format!("{} {}", e.key, node_text(&e.value))).collect())
            .unwrap_or_default();
        let shown = if have.is_empty() { "none set".to_owned() } else { have.join(", ") };
        return Ok(ok(w, M_DESCRIBE, &[&npc_name, "attribute", &shown]));
    };
    let key = raw.to_ascii_lowercase();
    let key = key.strip_prefix("minecraft:").unwrap_or(&key).to_owned();
    let Some(value) = p.words.get(1) else {
        return Ok(match saved(w, id, "attribute", &key) {
            Some(n) => ok(w, M_SET, &[&npc_name, &format!("attribute {key}"), &node_text(&n)]),
            None => ok(w, M_NO_ENTRY, &["attribute", &key]),
        });
    };
    let (node, shown) = check(w, &key, V::Num(NUM.0, NUM.1), value)?;
    {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "attribute", &key], node);
        npc.add_trait_name("attribute");
    }
    let out = ok(w, M_SET, &[&npc_name, &format!("attribute {key}"), &shown]);
    Ok(finish(w, id, false, out))
}

fn string_list(w: &World, id: i32, key: &str) -> Vec<String> {
    w.saves.get(id).and_then(|n| n.map.at(&["traits", "playerfilter", key])).map(Node::str_list).unwrap_or_default()
}

/// `/npc playerfilter`: the mode (`-a`, `-d`, `-e`), the lists and `-c` to clear them. Saved only:
/// Horizon has no per-player entity visibility yet.
fn playerfilter(w: &mut World, id: i32, m: &'static Mob, p: &Parsed) -> Res {
    let npc_name = name_at(w, id);
    if p.opts.is_empty() && p.flags.is_empty() {
        let mode = saved(w, id, "playerfilter", "mode").and_then(|n| n.text()).unwrap_or_else(|| "EMPTY".into());
        let line = format!(
            "mode {mode}, players {}, permissions {}, groups {}",
            string_list(w, id, "players").len(),
            string_list(w, id, "permissions").len(),
            string_list(w, id, "groups").len()
        );
        return Ok(ok(w, M_DESCRIBE, &[&npc_name, "playerfilter", &line]));
    }
    let mut out = Vec::new();
    for fl in &p.flags {
        let mode = match fl.ch {
            'a' => Some("ALLOWLIST"),
            'd' => Some("DENYLIST"),
            'e' => Some("EMPTY"),
            _ => None,
        };
        if let Some(mode) = mode {
            npc_mut(w, id)?.map.put(&["traits", "playerfilter", "mode"], Node::str(mode));
            out.extend(ok(w, M_SET, &[&npc_name, "playerfilter mode", mode]));
        } else {
            let npc = npc_mut(w, id)?;
            for key in ["players", "permissions", "groups"] {
                npc.map.put(&["traits", "playerfilter", key], Node::seq(Vec::new()));
            }
            out.extend(ok(w, M_SET, &[&npc_name, "playerfilter lists", "empty"]));
        }
    }
    for (key, _, raw) in &p.opts {
        let (list, adding) = match *key {
            "add" => ("players", true),
            "remove" => ("players", false),
            "addpermission" => ("permissions", true),
            "removepermission" => ("permissions", false),
            "addgroup" => ("groups", true),
            "removegroup" => ("groups", false),
            _ => {
                let (node, shown) = check(w, key, V::Num(NUM0.0, NUM0.1), raw)?;
                npc_mut(w, id)?.map.put(&["traits", "playerfilter", "applywithin"], node);
                out.extend(ok(w, M_SET, &[&npc_name, "playerfilter applywithin", &shown]));
                continue;
            }
        };
        let value = if list == "players" {
            match crate::horizon_citizens_core::ids::parse_uuid(raw) {
                Some(u) => crate::horizon_citizens_core::ids::format_uuid(u),
                None => return Err(err(w, M_UNKNOWN_NAME, &[raw, "UUID"])),
            }
        } else {
            raw.clone()
        };
        let mut items = string_list(w, id, list);
        if adding {
            if !items.contains(&value) {
                items.push(value.clone());
            }
        } else if let Some(i) = items.iter().position(|x| *x == value) {
            items.remove(i);
        } else {
            return Err(err(w, M_NO_ENTRY, &[list, &value]));
        }
        npc_mut(w, id)?.map.put(&["traits", "playerfilter", list], Node::seq(items.iter().map(|s| Node::str(s)).collect()));
        out.extend(ok(w, M_SET, &[&npc_name, &format!("playerfilter {list}"), &format!("{} {value}", if adding { "added" } else { "removed" })]));
    }
    npc_mut(w, id)?.add_trait_name("playerfilter");
    let _ = m;
    Ok(finish(w, id, false, out))
}

/// `/npc potioneffect [add|remove|list]`: effects saved under `traits.potioneffect.<name>`; the
/// name defaults to the type. `-t` (temporary) is accepted and means the same.
fn potioneffect(w: &mut World, id: i32, p: &Parsed) -> Res {
    let npc_name = name_at(w, id);
    let action = p.words.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let opt = |k: &str| p.opts.iter().find(|(key, _, _)| *key == k).map(|(_, _, v)| v.clone());
    match action.as_str() {
        "list" => {
            let have: Vec<String> = w
                .saves
                .get(id)
                .and_then(|n| n.map.at(&["traits", "potioneffect"]))
                .map(|n| {
                    n.entries()
                        .iter()
                        .map(|e| {
                            let t = |k: &str| e.value.get(k).map(node_text).unwrap_or_default();
                            format!("{} ({} for {} ticks, level {})", e.key, t("type"), t("duration"), t("amplifier"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let shown = if have.is_empty() { "no potion effects".to_owned() } else { have.join(", ") };
            Ok(ok(w, M_LISTED, &[&npc_name, &shown]))
        }
        "add" => {
            let Some(kind) = opt("type") else {
                return Err(usage_of(w, "potioneffect"));
            };
            let kind = kind.to_ascii_lowercase();
            let kind = kind.strip_prefix("minecraft:").unwrap_or(&kind).to_owned();
            let name = opt("name").unwrap_or_else(|| kind.clone());
            let duration = check(w, "duration", V::Int(-1, IMAX), &opt("duration").unwrap_or_else(|| "600".into()))?.0;
            let amplifier = check(w, "amplifier", V::Int(0, 255), &opt("amplifier").unwrap_or_else(|| "0".into()))?.0;
            let icon = check(w, "icon", V::Bool, &opt("icon").unwrap_or_else(|| "true".into()))?.0;
            {
                let npc = npc_mut(w, id)?;
                npc.map.put(&["traits", "potioneffect", &name, "type"], Node::str(&kind));
                npc.map.put(&["traits", "potioneffect", &name, "duration"], duration);
                npc.map.put(&["traits", "potioneffect", &name, "amplifier"], amplifier);
                npc.map.put(&["traits", "potioneffect", &name, "icon"], icon);
                npc.add_trait_name("potioneffect");
            }
            let out = ok(w, M_SET, &[&npc_name, "potioneffect", &name]);
            Ok(finish(w, id, false, out))
        }
        "remove" => {
            let wanted = opt("name").or_else(|| opt("type")).map(|s| s.to_ascii_lowercase());
            let Some(wanted) = wanted else {
                return Err(usage_of(w, "potioneffect"));
            };
            let wanted = wanted.strip_prefix("minecraft:").unwrap_or(&wanted).to_owned();
            if npc_mut(w, id)?.map.drop_path(&["traits", "potioneffect", &wanted]).is_none() {
                return Err(err(w, M_NO_ENTRY, &["potion effect", &wanted]));
            }
            w.mark_dirty();
            Ok(ok(w, M_SET, &[&npc_name, "potioneffect", &format!("{wanted} removed")]))
        }
        _ => Err(usage_of(w, "potioneffect")),
    }
}

/// `/npc speak [message] --target [player name] --range [blocks]`: the message in the talk
/// format, to the named player or to the players within range of the NPC (5 blocks without
/// `--range`). `--bubble` (a hologram line) is accepted and not shown.
fn speak(w: &mut World, sender: &Sender, id: i32, tail: &str) -> Res {
    // The message is everything before the first `--option`.
    let message: Vec<&str> = tail.split_whitespace().take_while(|t| !t.starts_with("--")).collect();
    if message.is_empty() {
        return Err(usage_of(w, "speak"));
    }
    let p = parse(row("speak").ok_or_else(Vec::new)?, tail).map_err(|_| usage_of(w, "speak"))?;
    let opt = |k: &str| p.opts.iter().find(|(key, _, _)| *key == k).map(|(_, _, v)| v.clone());
    let range = match opt("range") {
        Some(r) => check(w, "range", V::Num(NUM0.0, NUM0.1), &r)?.0.as_f64().unwrap_or(SPEAK_RANGE),
        None => SPEAK_RANGE,
    };
    let (npc_name, at, dim) = {
        let name = w.saves.get(id).map(NpcData::name).unwrap_or_default();
        let at = w.current_location(id).map(|l| [l.x, l.y, l.z]);
        (name, at, w.dimension_of(id))
    };
    let mut heard: Vec<u128> = Vec::new();
    if let Some(who) = opt("target") {
        if let Some(u) = w.player_by_name(&who) {
            heard.push(u);
        } else if who.parse::<i32>().is_err() {
            return Err(err(w, M_UNKNOWN_NAME, &[&who, "player"]));
        }
    } else if let (Some(at), Some(dim)) = (at, dim) {
        heard.extend(w.online.iter().filter(|pl| pl.dimension == dim && look::dist2(pl.pos, at) <= range * range).map(|pl| pl.uuid));
    }
    if heard.is_empty() {
        return Ok(err(w, M_NO_TARGET, &[&dec2(range)]));
    }
    let text = w.cfg.talk.format.replace("<npc>", &npc_name).replace("<text>", &message.join(" "));
    let legacy = w.cfg.messages.format(&text);
    for player in heard {
        w.push_op(Op::Tell { player, legacy: legacy.clone() });
    }
    let _ = sender;
    Ok(Vec::new())
}

/// `/npc panimate [animation]`: the arm swings play; the others have no console command.
fn panimate(w: &mut World, id: i32, p: &Parsed) -> Res {
    let Some(raw) = p.words.first() else {
        return Err(usage_of(w, "panimate"));
    };
    let word = raw.to_ascii_lowercase();
    if !ANIMATIONS.contains(&word.as_str()) {
        return Err(err(w, M_PANIMATE_INVALID, &[&ANIMATIONS.join(", ").to_ascii_uppercase()]));
    }
    let (Some(uuid), Some(dim)) = (w.saves.get(id).and_then(NpcData::uuid), w.dimension_of(id)) else {
        return Err(err(w, M_NPC_NOT_FOUND, &[]));
    };
    let hand = match word.as_str() {
        "arm_swing" => "mainhand",
        "arm_swing_offhand" => "offhand",
        _ => return Err(err(w, M_PANIMATE_UNSUPPORTED, &[&word.to_ascii_uppercase()])),
    };
    if w.is_out(id) {
        w.push_op(Op::Console(format!("execute in {dim} run swing {} {hand}", crate::horizon_citizens_core::ids::format_uuid(uuid))));
    }
    Ok(Vec::new())
}

/// `/npc undo (all)`: takes back the last create or remove (all of them with `all`).
fn undo(w: &mut World, sender: &Sender, p: &Parsed) -> Res {
    let _ = sender;
    let all = p.words.first().is_some_and(|s| s.eq_ignore_ascii_case("all"));
    if p.words.len() > 1 || (!p.words.is_empty() && !all) {
        return Err(usage_of(w, "undo"));
    }
    let mut out = Vec::new();
    loop {
        let Some(what) = w.undo_last() else {
            if out.is_empty() {
                return Err(err(w, M_UNDO_NOTHING, &[]));
            }
            break;
        };
        out.extend(ok(w, M_UNDONE, &[&what]));
        if !all {
            break;
        }
    }
    Ok(out)
}

// ---- the entity ----------------------------------------------------------------------------------

fn tbool(npc: &NpcData, name: &str, key: &str) -> Option<bool> {
    npc.map.at(&["traits", name, key]).and_then(Node::as_bool)
}

fn tint(npc: &NpcData, name: &str, key: &str) -> Option<i64> {
    npc.map.at(&["traits", name, key]).and_then(Node::as_i64)
}

fn tnum_list(npc: &NpcData, name: &str, key: &str) -> Option<Vec<f64>> {
    let raw = npc.map.at(&["traits", name, key]).and_then(Node::text)?;
    let v: Vec<f64> = raw.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    (v.len() == 3).then_some(v)
}

fn float_nbt(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0');
    let s = s.strip_suffix('.').unwrap_or(s);
    format!("{s}f")
}

/// The settings of this file that change the summoned entity, as the NBT keys `/summon` takes
/// (appended to the NPC's own). `kind` is the entity id (`minecraft:armor_stand`). Keys the NPC's
/// own data already writes (`NoGravity`, `Invulnerable`) are not written twice.
pub fn live_nbt(npc: &NpcData, kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    match kind {
        "minecraft:armor_stand" => {
            let t = |k: &str| tbool(npc, "armorstand", k);
            if t("visible") == Some(false) {
                out.push("Invisible:1b".into());
            }
            if t("small") == Some(true) {
                out.push("Small:1b".into());
            }
            if t("marker") == Some(true) {
                out.push("Marker:1b".into());
            }
            if t("arms") == Some(true) {
                out.push("ShowArms:1b".into());
            }
            if t("baseplate") == Some(false) {
                out.push("NoBasePlate:1b".into());
            }
            if t("gravity") == Some(false) && !npc.no_gravity() {
                out.push("NoGravity:1b".into());
            }
            let poses: Vec<String> = [("headpose", "Head"), ("bodypose", "Body"), ("leftarmpose", "LeftArm"), ("leftlegpose", "LeftLeg"), ("rightarmpose", "RightArm"), ("rightlegpose", "RightLeg")]
                .iter()
                .filter_map(|(key, nbt)| {
                    let v = tnum_list(npc, "armorstand", key)?;
                    Some(format!("{nbt}:[{},{},{}]", float_nbt(v[0]), float_nbt(v[1]), float_nbt(v[2])))
                })
                .collect();
            if !poses.is_empty() {
                out.push(format!("Pose:{{{}}}", poses.join(",")));
            }
        }
        "minecraft:phantom" => {
            if let Some(size) = tint(npc, "phantom", "size") {
                out.push(format!("size:{size}"));
            }
        }
        "minecraft:pufferfish" => {
            if let Some(state) = tint(npc, "pufferfish", "state") {
                out.push(format!("PuffState:{state}"));
            }
        }
        "minecraft:text_display" => {
            if let Some(text) = npc.map.at(&["traits", "textdisplay", "text"]).and_then(Node::text) {
                out.push(format!("text:{}", crate::horizon_citizens_core::text::component_snbt(&text)));
            }
            if let Some(b) = tbool(npc, "textdisplay", "shadowed") {
                out.push(format!("shadow:{}b", u8::from(b)));
            }
            if let Some(b) = tbool(npc, "textdisplay", "seethrough") {
                out.push(format!("see_through:{}b", u8::from(b)));
            }
            if let Some(width) = tint(npc, "textdisplay", "line_width") {
                out.push(format!("line_width:{width}"));
            }
        }
        "minecraft:villager" | "minecraft:zombie_villager" => {
            let id_of = |key: &str| {
                npc.map
                    .at(&["traits", "villager", key])
                    .and_then(Node::text)
                    .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                    .map(|s| format!("\"minecraft:{}\"", s.to_ascii_lowercase()))
            };
            let mut parts = Vec::new();
            if let Some(level) = tint(npc, "villager", "level") {
                parts.push(format!("level:{level}"));
            }
            if let Some(t) = id_of("type") {
                parts.push(format!("type:{t}"));
            }
            if let Some(p) = id_of("profession") {
                parts.push(format!("profession:{p}"));
            }
            if !parts.is_empty() {
                out.push(format!("VillagerData:{{{}}}", parts.join(",")));
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_citizens_core::config::Config;

    fn setup(kind: &str) -> (World, Sender) {
        let mut cfg = Config::default();
        cfg.skins.fetch = false;
        let mut w = World::new(cfg, Saves::empty(), None);
        let s = Sender {
            uuid: Some(0xA1),
            name: "Alex".into(),
            permission: 2,
            grants: Vec::new(),
            at: Some((
                "minecraft:overworld".into(),
                Location { world: "world".into(), x: 1.0, y: 64.0, z: 1.0, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 },
            )),
        };
        super::super::run(&mut w, &s, &format!("npc create Bob --type {kind}"), || Ok(Saves::empty()));
        (w, s)
    }

    fn cmd(w: &mut World, s: &Sender, line: &str) -> Vec<String> {
        super::super::run(w, s, line, || Ok(Saves::empty()))
    }

    fn saved_of(w: &World, name: &str, key: &str) -> Option<Node> {
        w.saves.get(0).unwrap().map.at(&["traits", name, key]).cloned()
    }

    #[test]
    fn every_row_has_a_listed_node_and_tab_tails() {
        for m in MOBS {
            assert_eq!(m.cmd.node, format!("citizens.npc.{}", m.cmd.name));
            assert!(!tails(m).is_empty());
        }
        assert_eq!(nodes().len(), MOBS.len());
    }

    #[test]
    fn wolf_flags_and_options_are_saved() {
        let (mut w, s) = setup("wolf");
        let r = cmd(&mut w, &s, "npc wolf -s -a --collar red --variant ashen");
        assert!(r.iter().any(|l| l == "&a&eBob&a's wolf sitting set to &etrue&a."), "{r:?}");
        assert!(r.iter().any(|l| l == "&a&eBob&a's wolf collar set to &ered&a."), "{r:?}");
        assert!(r.last().unwrap().contains("does not act on it"), "{r:?}");
        assert_eq!(saved_of(&w, "wolf", "angry").and_then(|n| n.as_bool()), Some(true));
        assert_eq!(saved_of(&w, "wolf", "variant").and_then(|n| n.text()).as_deref(), Some("ashen"));
        let r = cmd(&mut w, &s, "npc wolf");
        assert!(r[0].contains("settings:") && r[0].contains("collar red"), "{r:?}");
    }

    #[test]
    fn a_wrong_mob_type_is_refused() {
        let (mut w, s) = setup("zombie");
        let r = cmd(&mut w, &s, "npc wolf -s");
        assert!(r[0].contains("cannot be the mob type"), "{r:?}");
        assert!(saved_of(&w, "wolf", "sitting").is_none());
    }

    #[test]
    fn values_are_checked() {
        let (mut w, s) = setup("armor_stand");
        let r = cmd(&mut w, &s, "npc armorstand --small maybe");
        assert!(r[0].contains("not a valid value"), "{r:?}");
        let r = cmd(&mut w, &s, "npc armorstand --headpose 1,2");
        assert!(r[0].contains("not a valid value"), "{r:?}");
        let r = cmd(&mut w, &s, "npc armorstand --headpose 10,-20.5,0 --small true");
        assert!(r.iter().any(|l| l.contains("armorstand headpose set to &e10,-20.5,0")), "{r:?}");
        assert!(!r.iter().any(|l| l.contains("does not act")), "armor stands are live: {r:?}");
        let r = cmd(&mut w, &s, "npc armorstand --bogus 1");
        assert!(r[0].contains("armorstand"), "{r:?}");
    }

    #[test]
    fn numbers_and_ranges() {
        let (mut w, s) = setup("phantom");
        let r = cmd(&mut w, &s, "npc phantom --size 99");
        assert!(r[0].contains("must be between"), "{r:?}");
        cmd(&mut w, &s, "npc phantom --size 7");
        assert_eq!(saved_of(&w, "phantom", "size").and_then(|n| n.as_i64()), Some(7));
        let npc = w.saves.get(0).unwrap();
        assert_eq!(live_nbt(npc, "minecraft:phantom"), vec!["size:7".to_owned()]);
    }

    #[test]
    fn a_required_word_and_a_text_value() {
        let (mut w, s) = setup("rabbit");
        let r = cmd(&mut w, &s, "npc rabbittype");
        assert!(r[0].contains("/npc rabbittype"), "{r:?}");
        let r = cmd(&mut w, &s, "npc rbtype gold");
        assert!(r[0].contains("rabbittype type set to &eGOLD"), "{r:?}");
        let r = cmd(&mut w, &s, "npc rabbittype purple");
        assert!(r.iter().any(|l| l.contains("Valid values are")), "{r:?}");
        let (mut w, s) = setup("text_display");
        cmd(&mut w, &s, "npc textdisplay --text Hello there world --shadowed true");
        assert_eq!(saved_of(&w, "textdisplay", "text").and_then(|n| n.text()).as_deref(), Some("Hello there world"));
        let nbt = live_nbt(w.saves.get(0).unwrap(), "minecraft:text_display");
        assert!(nbt.contains(&"shadow:1b".to_owned()), "{nbt:?}");
    }

    #[test]
    fn controllable_toggles_and_flags_with_two_keys() {
        let (mut w, s) = setup("goat");
        cmd(&mut w, &s, "npc goat -l");
        assert_eq!(saved_of(&w, "goat", "left_horn").and_then(|n| n.as_bool()), Some(true));
        cmd(&mut w, &s, "npc goat -n");
        assert_eq!(saved_of(&w, "goat", "left_horn").and_then(|n| n.as_bool()), Some(false));
        assert_eq!(saved_of(&w, "goat", "right_horn").and_then(|n| n.as_bool()), Some(false));
        let r = cmd(&mut w, &s, "npc controllable");
        assert!(r[0].contains("controllable is now"), "{r:?}");
        let r = cmd(&mut w, &s, "npc control");
        assert!(r[0].contains("disabled"), "{r:?}");
    }

    #[test]
    fn potion_effects_and_attributes() {
        let (mut w, s) = setup("zombie");
        let r = cmd(&mut w, &s, "npc potioneffect add --type speed --duration 100 --amplifier 1");
        assert!(r[0].contains("potioneffect set to &espeed"), "{r:?}");
        let r = cmd(&mut w, &s, "npc potioneffect list");
        assert!(r[0].contains("speed (speed for 100 ticks, level 1)"), "{r:?}");
        let r = cmd(&mut w, &s, "npc potioneffect remove --name speed");
        assert!(r[0].contains("speed removed"), "{r:?}");
        let r = cmd(&mut w, &s, "npc potioneffect remove --name speed");
        assert!(r[0].contains("No"), "{r:?}");
        let r = cmd(&mut w, &s, "npc attribute generic.max_health 40");
        assert!(r[0].contains("attribute generic.max_health set to &e40"), "{r:?}");
        let r = cmd(&mut w, &s, "npc attribute generic.max_health");
        assert!(r[0].contains("&e40"), "{r:?}");
    }

    #[test]
    fn the_player_filter_keeps_lists() {
        let (mut w, s) = setup("zombie");
        let uuid = "11111111-2222-3333-4444-555555555555";
        cmd(&mut w, &s, &format!("npc playerfilter -a --add {uuid} --addgroup vip"));
        let r = cmd(&mut w, &s, "npc playerfilter");
        assert!(r[0].contains("mode ALLOWLIST, players 1, permissions 0, groups 1"), "{r:?}");
        let r = cmd(&mut w, &s, "npc playerfilter --add nonsense");
        assert!(r[0].contains("not a valid"), "{r:?}");
        cmd(&mut w, &s, "npc playerfilter -c");
        let r = cmd(&mut w, &s, "npc playerfilter");
        assert!(r[0].contains("players 0"), "{r:?}");
    }

    #[test]
    fn speak_reaches_players_in_range() {
        let (mut w, s) = setup("zombie");
        w.online = vec![
            look::PlayerView { uuid: 7, name: "Near".into(), dimension: "minecraft:overworld".into(), pos: [2.0, 64.0, 2.0], game_mode: 0, permission: 0, entity_id: 1, sneaking: false },
            look::PlayerView { uuid: 8, name: "Far".into(), dimension: "minecraft:overworld".into(), pos: [90.0, 64.0, 2.0], game_mode: 0, permission: 0, entity_id: 2, sneaking: false },
        ];
        w.spawn(0);
        let _ = w.take_ops();
        let r = cmd(&mut w, &s, "npc speak Hello there --bubble 40");
        assert!(r.is_empty(), "{r:?}");
        let ops = w.take_ops();
        let told: Vec<u128> = ops.iter().filter_map(|o| if let Op::Tell { player, .. } = o { Some(*player) } else { None }).collect();
        assert_eq!(told, vec![7]);
        assert!(ops.iter().any(|o| matches!(o, Op::Tell { legacy, .. } if legacy.contains("Hello there"))));
        let r = cmd(&mut w, &s, "npc speak");
        assert!(r[0].contains("/npc speak"), "{r:?}");
    }

    #[test]
    fn panimate_swings_and_undo_takes_back_a_remove() {
        let (mut w, s) = setup("player");
        w.spawn(0);
        let _ = w.take_ops();
        cmd(&mut w, &s, "npc panimate arm_swing");
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Console(c) if c.contains(" swing ") && c.ends_with("mainhand"))));
        let r = cmd(&mut w, &s, "npc panimate sneak");
        assert!(r[0].contains("not supported"), "{r:?}");
        let r = cmd(&mut w, &s, "npc panimate nonsense");
        assert!(r[0].contains("Valid animations"), "{r:?}");
        cmd(&mut w, &s, "npc remove 0");
        assert!(w.saves.get(0).is_none());
        let r = cmd(&mut w, &s, "npc undo");
        assert!(r[0].contains("Undid the last action"), "{r:?}");
        assert_eq!(w.saves.get(0).map(NpcData::kind).as_deref(), Some("PLAYER"));
        let r = cmd(&mut w, &s, "npc undo");
        assert!(r[0].contains("Undid"), "{r:?}");
        assert!(w.saves.get(0).is_none(), "undoing the create removes it");
        let r = cmd(&mut w, &s, "npc undo");
        assert!(r[0].contains("nothing to undo"), "{r:?}");
    }

    #[test]
    fn villager_data_reaches_the_entity() {
        let (mut w, s) = setup("villager");
        cmd(&mut w, &s, "npc villager --level 3 --profession farmer --type snow");
        let nbt = live_nbt(w.saves.get(0).unwrap(), "minecraft:villager");
        assert_eq!(nbt, vec!["VillagerData:{level:3,type:\"minecraft:snow\",profession:\"minecraft:farmer\"}".to_owned()]);
    }
}
