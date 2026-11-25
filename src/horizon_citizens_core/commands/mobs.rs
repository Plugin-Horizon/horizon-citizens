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
    /// Saved keys that only show in synched data the entity does not save (reply says so).
    noeffect: &'static [&'static str],
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
            noeffect: &[],
        }
    };
}

const MOBS: &[Mob] = &[
    Mob { kinds: &["ALLAY"], flags: &[f('d', "dancing", true)], ..m!("allay", [], "allay (-d(ancing))") },
    Mob {
        kinds: &["AREA_EFFECT_CLOUD"],
        opts: &[o("color", V::Any), o("duration", V::Int(0, IMAX)), o("radius", V::Num(NUM0.0, NUM0.1)), o("radius_per_tick", V::Num(NUM.0, NUM.1)), o("particle", V::Any)],
        live: true, ..m!("areaeffectcloud", [], "areaeffectcloud --color [color] --duration [duration] --radius [radius] --radius_per_tick [radius] --particle [particle]")
    },
    Mob { kinds: &["ARMADILLO"], opts: &[o("state", V::Words(&["idle", "rolling", "scared", "unrolling"]))], live: true, ..m!("armadillo", [], "armadillo --state [state]") },
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
        live: true, noeffect: &["d"], ..m!("axolotl", [], "axolotl (-d) (--variant variant)")
    },
    Mob { kinds: &["BAT"], opts: &[o("awake", V::Bool)], live: true, ..m!("bat", [], "bat --awake [awake]") },
    Mob {
        kinds: &["BEE"],
        flags: &[f('s', "stung", true), f('n', "stung", false)],
        opts: &[o("anger", V::Int(0, IMAX))],
        live: true, ..m!("bee", [], "bee (-s/-n) --anger anger")
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
    Mob { kinds: &["CAMEL"], opts: &[o("pose", V::Any)], live: true, ..m!("camel", [], "camel (--pose pose)") },
    Mob {
        kinds: &["CAT"],
        flags: &[f('s', "sitting", true), f('n', "sitting", false), f('l', "lying", true)],
        opts: &[o("type", V::Any), o("ccolor", V::Any)],
        live: true, noeffect: &["lying"], ..m!("cat", [], "cat (-s/-n/-l) --type type --ccolor collar color")
    },
    Mob { kinds: &["CHICKEN"], opts: &[o("variant", V::Any)], live: true, ..m!("chicken", [], "chicken (--variant variant)") },
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
    Mob { kinds: &["COW"], opts: &[o("variant", V::Any)], live: true, ..m!("cow", [], "cow (--variant variant)") },
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
        live: true, ..m!("display", [], "display --billboard [billboard] --brightness [blockLight,skyLight] --interpolationdelay [delay] --interpolationduration [duration] --height [height] --width [width] --scale [x,y,z] --viewrange [range] --leftrotation [x,y,z,w] --rightrotation [x,y,z,w] --offset [x,y,z]")
    },
    Mob { kinds: &["END_CRYSTAL", "ENDER_CRYSTAL"], flags: &[f('b', "bottom", true)], live: true, ..m!("endercrystal", [], "endercrystal -b(ottom)") },
    Mob {
        kinds: &["ENDER_DRAGON"],
        opts: &[o("phase", V::Any), o("destroywalls", V::Bool)],
        live: true, ..m!("enderdragon", [], "enderdragon --phase [phase] --destroywalls [true|false]")
    },
    Mob { kinds: &["ENDERMAN"], flags: &[f('a', "angry", true)], noeffect: &["angry"], ..m!("enderman", [], "enderman -a(ngry)") },
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
        live: true, ..m!("fox", [], "fox --type type --sleeping [true|false] --sitting [true|false] --crouching [true|false] --interested [true|false] --pouncing [true|false] --faceplanted [true|false]")
    },
    Mob { kinds: &["FROG"], opts: &[o("variant", V::Any)], live: true, ..m!("frog", [], "frog (--variant variant)") },
    Mob {
        kinds: &["GOAT"],
        flags: &[
            f('l', "left_horn", true),
            f('r', "right_horn", true),
            Flag { ch: 'n', key: "left_horn", val: false, key2: "right_horn" },
            Flag { ch: 'b', key: "left_horn", val: true, key2: "right_horn" },
        ],
        pos: &[o("horn", V::Any)],
        live: true, ..m!("goat", [], "goat -l(eft) -r(ight) -n(either) -b(oth) horn")
    },
    Mob {
        opts: &[o("scale", V::Num(NUM0.0, NUM0.1)), o("width", V::Num(NUM0.0, NUM0.1)), o("height", V::Num(NUM0.0, NUM0.1)), o("offset", V::Vec3)],
        live: true, ..m!("hitbox", [], "hitbox --scale [scale] --width/height [value] --offset [x,y,z]")
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
        live: true, ..m!("horse", [], "horse (--color color) (--type type) (--style style) (-cbt)")
    },
    Mob { flags: &[f('h', "hand", true)], pos: &[o("item", V::Any)], ..m!("item", [], "item [item] (-h(and))") },
    Mob { kinds: &["ITEM_DISPLAY"], opts: &[o("transform", V::Any)], live: true, ..m!("itemdisplay", [], "itemdisplay --transform [transform]") },
    Mob {
        kinds: &["ITEM_FRAME", "GLOW_ITEM_FRAME"],
        opts: &[o("visible", V::Bool), o("fixed", V::Bool), o("rotation", V::Any), o("item", V::Any), o("face", V::Any)],
        live: true, ..m!("itemframe", [], "itemframe --visible [true|false] --fixed [true|false] --rotation [rotation] --item [item] --face [face]")
    },
    Mob {
        kinds: &["LLAMA", "TRADER_LLAMA"],
        opts: &[o("color", V::Any), o("strength", V::Int(1, 5))],
        live: true, ..m!("llama", [], "llama (--color color) (--strength strength)")
    },
    Mob {
        kinds: &["MANNEQUIN", "PLAYER"],
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
        live: true, noeffect: &["skin_patch_type", "skin_patch_body", "skin_patch_cape", "skin_patch_elytra"], ..m!("mannequin", [], "mannequin --hide_description [true|false] --immovable [true|false] --description [description] --main_hand [LEFT|RIGHT] --skin_patch_type [type] --skin_patch_body [location] --skin_patch_cape [location] --skin_patch_elytra [location]")
    },
    Mob { kinds: &["*MINECART"], opts: &[o("offset", V::Int(-IMAX, IMAX))], live: true, ..m!("minecart", [], "minecart (--offset offset)") },
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
    Mob { kinds: &["MUSHROOM_COW", "MOOSHROOM"], opts: &[o("variant", V::Any)], live: true, ..m!("mushroomcow", [], "mushroomcow (--variant [variant])") },
    Mob {
        kinds: &["OCELOT"],
        flags: &[f('s', "sitting", true), f('n', "sitting", false)],
        opts: &[o("type", V::Any)],
        ..m!("ocelot", [], "ocelot (--type type) (-s(itting), -n(ot sitting))")
    },
    Mob { opts: &[o("enabled", V::Bool)], ..m!("packet", [], "packet --enabled [true|false]") },
    Mob { kinds: &["PAINTING"], opts: &[o("art", V::Any)], live: true, ..m!("painting", [], "painting (--art art)") },
    Mob {
        kinds: &["PANDA"],
        flags: &[f('e', "eating", true), f('s', "sitting", true), f('n', "sneezing", true), f('r', "rolling", true)],
        opts: &[o("gene", V::Any), o("hiddengene", V::Any)],
        live: true, noeffect: &["eating"], ..m!("panda", [], "panda --gene (main gene) --hiddengene (hidden gene) -e(ating) -s(itting) -n (sneezing) -r(olling)")
    },
    Mob { kinds: &["PARROT"], opts: &[o("variant", V::Any)], live: true, ..m!("parrot", [], "parrot (--variant variant)") },
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
    Mob { kinds: &["PIG"], opts: &[o("variant", V::Any)], live: true, ..m!("pig", [], "pig (--variant variant)") },
    Mob { kinds: &["PIGLIN"], opts: &[o("dancing", V::Bool)], noeffect: &["dancing"], ..m!("piglin", [], "piglin (--dancing [true|false])") },
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
        live: true, ..m!("rabbittype", ["rbtype"], "rabbittype [type]")
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
        live: true, ..m!("shulker", [], "shulker (--peek [peek] --color [color])")
    },
    Mob { kinds: &["SNIFFER"], opts: &[o("state", V::Any)], noeffect: &["state"], ..m!("sniffer", [], "sniffer (--state [state])") },
    Mob {
        kinds: &["SNOWMAN", "SNOW_GOLEM"],
        flags: &[f('d', "derp", true), f('f', "form_snow", true)],
        live: true, noeffect: &["form_snow"], ..m!("snowman", [], "snowman (-d[erp]) (-f[orm snow])")
    },
    Mob { kinds: &["EVOKER", "ILLUSIONER"], opts: &[o("spell", V::Any)], noeffect: &["spell"], ..m!("spellcaster", [], "spellcaster (--spell spell)") },
    Mob {
        kinds: &["TEXT_DISPLAY"],
        opts: &[o("shadowed", V::Bool), o("seethrough", V::Bool), o("line_width", V::Int(0, IMAX)), o("text", V::Text)],
        live: true,
        ..m!("textdisplay", [], "textdisplay --shadowed [true|false] --seethrough [true|false] --line_width [width] --text [text]")
    },
    Mob {
        kinds: &["TROPICAL_FISH"],
        opts: &[o("body", V::Any), o("pattern", V::Any), o("patterncolor", V::Any)],
        live: true, ..m!("tropicalfish", [], "tropicalfish (--body color) (--pattern pattern) (--patterncolor color)")
    },
    Mob { flags: &[f('o', "offhand", true)], ..m!("useitem", [], "useitem (-o(ffhand))") },
    Mob { kinds: &["VEX"], opts: &[o("charging", V::Bool)], ..m!("vex", [], "vex (--charging [charging])") },
    Mob {
        kinds: &["VILLAGER", "ZOMBIE_VILLAGER"],
        flags: &[f('s', "shake_head", true)],
        opts: &[o("level", V::Int(1, 5)), o("type", V::Any), o("profession", V::Any)],
        live: true,
        noeffect: &["shake_head"], ..m!("villager", [], "villager (--level level) (--type type) (--profession profession) -s(hake head)")
    },
    Mob {
        kinds: &["WITHER"],
        opts: &[o("invulnerable", V::Bool), o("invulnerable-ticks", V::Int(0, IMAX)), o("arrow-shield", V::Bool)],
        live: true, ..m!("wither", [], "wither (--invulnerable [true|false]) (--invulnerable-ticks [ticks]) (--arrow-shield [true|false])")
    },
    Mob {
        kinds: &["WOLF"],
        flags: &[f('s', "sitting", true), f('a', "angry", true), f('t', "tamed", true), f('i', "interested", true)],
        opts: &[o("collar", V::Any), o("variant", V::Any)],
        live: true, noeffect: &["interested"], ..m!("wolf", [], "wolf (-s(itting) a(ngry) t(amed) i(nterested)) --collar [hex rgb color|name] --variant [variant]")
    },
    // The ones with their own handler: they read the line themselves.
    Mob { pos: &[o("attribute", V::Any), o("value", V::Num(NUM.0, NUM.1))], live: true, ..m!("attribute", [], "attribute [attribute] [value]") },
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
            return Ok(finish(w, id, m.live, false, out));
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
    // What only the host calls of `entity_set` can set reaches the entity at once, no respawn.
    if sets.iter().all(|(k, _, _)| hosted(name, k)) {
        w.mark_dirty();
        w.apply_settings(id);
        return Ok(out);
    }
    let partial = sets.iter().any(|(k, _, _)| m.noeffect.contains(k));
    Ok(finish(w, id, m.live, partial, out))
}

/// The saved keys that are set through the host calls of `entity_set`, not the summon data.
fn hosted(name: &str, key: &str) -> bool {
    matches!(
        (name, key),
        ("allay", "dancing")
            | ("fox", "interested" | "pouncing" | "faceplanted")
            | ("panda", "sneezing" | "rolling" | "sitting")
            | ("polarbear", "rearing")
            | ("vex", "charging")
            | ("enderdragon", "destroywalls")
    )
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

fn finish(w: &mut World, id: i32, live: bool, partial: bool, mut out: Reply) -> Reply {
    w.mark_dirty();
    if live {
        w.respawn(id);
    }
    if !live || partial {
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
    Ok(finish(w, id, true, false, out))
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
    Ok(finish(w, id, false, false, out))
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
            Ok(finish(w, id, true, false, out))
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
    let n = npc.map.at(&["traits", name, key])?;
    n.as_i64().or_else(|| n.text().and_then(|t| t.trim().parse().ok()))
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
    mob_nbt(npc, kind, &mut out);
    out
}

fn tstr(npc: &NpcData, name: &str, key: &str) -> Option<String> {
    let n = npc.map.at(&["traits", name, key])?;
    n.text().or_else(|| n.as_i64().map(|i| i.to_string())).map(|s| s.trim().to_ascii_lowercase().replace([' ', '-'], "_"))
}

fn tnum(npc: &NpcData, name: &str, key: &str) -> Option<f64> {
    let n = npc.map.at(&["traits", name, key])?;
    n.as_f64().or_else(|| n.text().and_then(|t| t.trim().parse().ok()))
}

/// `a,b,..` of exactly `n` numbers.
fn tlist(npc: &NpcData, name: &str, key: &str, n: usize) -> Option<Vec<f64>> {
    let raw = npc.map.at(&["traits", name, key]).and_then(Node::text)?;
    let v: Vec<f64> = raw.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    (v.len() == n).then_some(v)
}

fn tbool_key(npc: &NpcData, name: &str, key: &str) -> Option<bool> {
    tbool(npc, name, key)
}

fn b1(v: bool) -> u8 {
    u8::from(v)
}

/// A registry word as a namespaced id string, when it is only letters, digits and `_`.
fn ns_id(s: &str) -> Option<String> {
    let s = s.strip_prefix("minecraft:").unwrap_or(s);
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then(|| format!("\"minecraft:{s}\""))
}

fn index_of(list: &[&str], word: &str) -> Option<usize> {
    list.iter().position(|w| w.eq_ignore_ascii_case(word))
}

fn dye(word: &str) -> Option<usize> {
    index_of(&DYE_COLOURS, word)
}

/// The attribute id of a Citizens/Bukkit attribute name (`GENERIC_MAX_HEALTH`, `generic.max_health`).
fn attribute_id(raw: &str) -> Option<String> {
    let s = raw.to_ascii_lowercase().replace('.', "_");
    let mut s = s.strip_prefix("minecraft:").unwrap_or(&s);
    for p in ["generic_", "player_", "zombie_", "horse_"] {
        if let Some(r) = s.strip_prefix(p) {
            s = r;
            break;
        }
    }
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')).then(|| s.to_owned())
}

fn dbl(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0');
    let s = if s.ends_with('.') { format!("{s}0") } else { s.to_owned() };
    format!("{s}d")
}

/// The entity data of the mob-specific modifiers (the entity's own saved data; what the game
/// keeps only in synched state is listed in each row's `noeffect` and in ENGINE_NEEDS).
fn mob_nbt(npc: &NpcData, kind: &str, out: &mut Vec<String>) {
    let tb = |n: &str, k: &str| tbool(npc, n, k);
    let ts = |n: &str, k: &str| tstr(npc, n, k);
    let ti = |n: &str, k: &str| tint(npc, n, k);
    let id_of = |n: &str, k: &str| ts(n, k).and_then(|s| ns_id(&s));
    let mut attrs: Vec<String> = Vec::new();
    match kind {
        "minecraft:area_effect_cloud" => {
            let n = "areaeffectcloud";
            if let Some(v) = tnum(npc, n, "radius") {
                out.push(format!("Radius:{}", float_nbt(v)));
            }
            if let Some(v) = ti(n, "duration") {
                out.push(format!("Duration:{v}"));
            }
            if let Some(v) = tnum(npc, n, "radius_per_tick") {
                out.push(format!("RadiusPerTick:{}", float_nbt(v)));
            }
            if let Some(raw) = ts(n, "color") {
                let rgb = raw.trim_start_matches('#').trim_start_matches("0x");
                if let Ok(c) = i64::from_str_radix(rgb, 16) {
                    out.push(format!("custom_color:{}", c & 0xFF_FFFF));
                }
            }
            if let Some(p) = id_of(n, "particle") {
                out.push(format!("custom_particle:{{type:{p}}}"));
            }
        }
        "minecraft:armadillo" => {
            if let Some(s) = ts("armadillo", "state").filter(|s| ["idle", "rolling", "scared", "unrolling"].contains(&s.as_str())) {
                out.push(format!("state:\"{s}\""));
            }
        }
        "minecraft:axolotl" => {
            if let Some(i) = ts("axolotl", "variant").and_then(|v| index_of(&["lucy", "wild", "gold", "cyan", "blue"], &v)) {
                out.push(format!("Variant:{i}"));
            }
        }
        "minecraft:bat" => {
            if let Some(a) = tb("bat", "awake") {
                out.push(format!("BatFlags:{}b", b1(!a)));
            }
        }
        "minecraft:bee" => {
            if let Some(b) = tb("bee", "stung") {
                out.push(format!("HasStung:{}b", b1(b)));
            }
            if let Some(a) = ti("bee", "anger") {
                out.push(format!("AngerTime:{a}"));
            }
        }
        "minecraft:camel" => {
            if let Some(p) = ts("camel", "pose") {
                out.push(format!("LastPoseTick:{}L", if p.starts_with("sit") { -1 } else { 0 }));
            }
        }
        "minecraft:cat" => {
            if let Some(v) = id_of("cat", "type") {
                out.push(format!("variant:{v}"));
            }
            if let Some(c) = ts("cat", "ccolor").and_then(|c| dye(&c)) {
                out.push(format!("CollarColor:{c}b"));
            }
            if let Some(s) = tb("cat", "sitting") {
                out.push(format!("Sitting:{}b", b1(s)));
            }
        }
        "minecraft:chicken" | "minecraft:cow" | "minecraft:pig" | "minecraft:frog" => {
            let t = kind.trim_start_matches("minecraft:");
            if let Some(v) = id_of(t, "variant") {
                out.push(format!("variant:{v}"));
            }
        }
        "minecraft:end_crystal" => {
            if let Some(b) = tb("endercrystal", "bottom") {
                out.push(format!("ShowBottom:{}b", b1(b)));
            }
        }
        "minecraft:ender_dragon" => {
            const PHASES: [&str; 12] = [
                "circling", "strafing", "flying_to_portal", "landing_approach", "landing", "takeoff", "sitting_flaming", "sitting_scanning",
                "sitting_attacking", "charging_player", "dying", "hover",
            ];
            if let Some(i) = ts("enderdragon", "phase").and_then(|p| index_of(&PHASES, &p)) {
                out.push(format!("DragonPhase:{i}"));
            }
        }
        "minecraft:fox" => {
            if let Some(t) = ts("fox", "type").filter(|t| t == "red" || t == "snow") {
                out.push(format!("Type:\"{t}\""));
            }
            for (key, nbt) in [("sleeping", "Sleeping"), ("sitting", "Sitting"), ("crouching", "Crouching")] {
                if let Some(b) = tb("fox", key) {
                    out.push(format!("{nbt}:{}b", b1(b)));
                }
            }
        }
        "minecraft:goat" => {
            for (key, nbt) in [("left_horn", "HasLeftHorn"), ("right_horn", "HasRightHorn")] {
                if let Some(b) = tb("goat", key) {
                    out.push(format!("{nbt}:{}b", b1(b)));
                }
            }
        }
        "minecraft:horse" | "minecraft:donkey" | "minecraft:mule" | "minecraft:skeleton_horse" | "minecraft:zombie_horse" => {
            if kind == "minecraft:horse" {
                const COLOURS: [&str; 7] = ["white", "creamy", "chestnut", "brown", "black", "gray", "dark_brown"];
                const STYLES: [&str; 5] = ["none", "white", "whitefield", "white_dots", "black_dots"];
                let c = ts("horse", "color").and_then(|c| index_of(&COLOURS, &c));
                let s = ts("horse", "style").and_then(|s| index_of(&STYLES, &s));
                if c.is_some() || s.is_some() {
                    out.push(format!("Variant:{}", c.unwrap_or(0) | (s.unwrap_or(0) << 8)));
                }
            }
            if let Some(b) = tb("horse", "chest") {
                out.push(format!("ChestedHorse:{}b", b1(b)));
            }
            if tb("horse", "t") == Some(true) {
                out.push("Tame:1b".into());
            }
        }
        "minecraft:item_frame" | "minecraft:glow_item_frame" => {
            let n = "itemframe";
            if let Some(b) = tb(n, "visible") {
                out.push(format!("Invisible:{}b", b1(!b)));
            }
            if let Some(b) = tb(n, "fixed") {
                out.push(format!("Fixed:{}b", b1(b)));
            }
            if let Some(r) = ti(n, "rotation") {
                out.push(format!("ItemRotation:{}b", r.rem_euclid(8)));
            }
            if let Some(i) = index_of(&["down", "up", "north", "south", "west", "east"], &ts(n, "face").unwrap_or_default()) {
                out.push(format!("Facing:{i}b"));
            }
            if let Some(item) = id_of(n, "item") {
                out.push(format!("Item:{{id:{item},count:1}}"));
            }
        }
        "minecraft:llama" | "minecraft:trader_llama" => {
            if let Some(i) = ts("llama", "color").and_then(|c| index_of(&["creamy", "white", "brown", "gray"], &c)) {
                out.push(format!("Variant:{i}"));
            }
            if let Some(s) = ti("llama", "strength") {
                out.push(format!("Strength:{s}"));
            }
        }
        "minecraft:mannequin" => {
            let n = "mannequin";
            if let Some(h) = ts(n, "main_hand").filter(|h| h == "left" || h == "right") {
                out.push(format!("main_hand:\"{h}\""));
            }
            if let Some(d) = npc.map.at(&["traits", n, "description"]).and_then(Node::text) {
                out.push(format!("description:{}", crate::horizon_citizens_core::text::component_snbt(&d)));
            }
        }
        k if k.ends_with("minecart") => {
            if let Some(o) = ti("minecart", "offset") {
                out.push(format!("DisplayOffset:{o}"));
            }
        }
        "minecraft:mooshroom" => {
            if let Some(t) = ts("mushroomcow", "variant").filter(|t| t == "red" || t == "brown") {
                out.push(format!("Type:\"{t}\""));
            }
        }
        "minecraft:painting" => {
            if let Some(v) = id_of("painting", "art") {
                out.push(format!("variant:{v}"));
            }
        }
        "minecraft:panda" => {
            for (key, nbt) in [("gene", "MainGene"), ("hiddengene", "HiddenGene")] {
                if let Some(g) = ts("panda", key).filter(|g| ["normal", "aggressive", "lazy", "worried", "playful", "weak", "brown"].contains(&g.as_str())) {
                    out.push(format!("{nbt}:\"{g}\""));
                }
            }
        }
        "minecraft:parrot" => {
            if let Some(i) = ts("parrot", "variant").and_then(|v| index_of(&["red_blue", "blue", "green", "yellow_blue", "gray"], &v)) {
                out.push(format!("Variant:{i}"));
            }
        }
        "minecraft:rabbit" => {
            let t = ts("rabbittype", "type").and_then(|t| {
                if t == "the_killer_bunny" {
                    Some(99)
                } else {
                    index_of(&["brown", "white", "black", "black_and_white", "gold", "salt_and_pepper"], &t)
                }
            });
            if let Some(t) = t {
                out.push(format!("RabbitType:{t}"));
            }
        }
        "minecraft:shulker" => {
            if let Some(p) = ti("shulker", "peek") {
                out.push(format!("Peek:{}b", p.clamp(0, 100)));
            }
            if let Some(c) = ts("shulker", "color").and_then(|c| dye(&c)) {
                out.push(format!("Color:{c}b"));
            }
        }
        "minecraft:snow_golem" => {
            if tb("snowman", "derp") == Some(true) {
                out.push("Pumpkin:0b".into());
            }
        }
        "minecraft:tropical_fish" => {
            // Small patterns are 0..5 (kob ...), large ones 256..261 (flopper ...); body colour << 16, pattern colour << 24.
            const PATTERNS: [&str; 12] = ["kob", "sunstreak", "snooper", "dasher", "brinely", "spotty", "flopper", "stripey", "glitter", "blockfish", "betty", "clayfish"];
            let n = "tropicalfish";
            let body = ts(n, "body").and_then(|c| dye(&c));
            let pc = ts(n, "patterncolor").and_then(|c| dye(&c));
            let pat = ts(n, "pattern").and_then(|p| index_of(&PATTERNS, &p));
            if body.is_some() || pc.is_some() || pat.is_some() {
                let p = pat.map_or(0, |i| if i < 6 { i as i64 } else { 256 + i as i64 - 6 });
                let v = p | ((body.unwrap_or(0) as i64) << 16) | ((pc.unwrap_or(0) as i64) << 24);
                out.push(format!("Variant:{}", v as i32));
            }
        }
        "minecraft:wither" => {
            if let Some(t) = ti("wither", "invulnerable-ticks") {
                out.push(format!("Invul:{t}"));
            }
        }
        "minecraft:wolf" => {
            let n = "wolf";
            if let Some(b) = tb(n, "sitting") {
                out.push(format!("Sitting:{}b", b1(b)));
            }
            if tb(n, "angry") == Some(true) {
                out.push("AngerTime:2147483647".into());
            }
            if let Some(c) = ts(n, "collar").and_then(|c| dye(&c)) {
                out.push(format!("CollarColor:{c}b"));
            }
            if let Some(v) = id_of(n, "variant") {
                out.push(format!("variant:{v}"));
            }
            if tb(n, "tamed") == Some(true)
                && let Some(owner) = npc.owner()
            {
                out.push(format!("Owner:{}", crate::horizon_citizens_core::ids::int_array_snbt(owner)));
            }
        }
        "minecraft:block_display" | "minecraft:item_display" | "minecraft:text_display" => {
            let n = "display";
            if let Some(b) = ts(n, "billboard").filter(|b| ["fixed", "vertical", "horizontal", "center"].contains(&b.as_str())) {
                out.push(format!("billboard:\"{b}\""));
            }
            if let Some(v) = tlist(npc, n, "brightness", 2) {
                out.push(format!("brightness:{{block:{},sky:{}}}", v[0] as i64, v[1] as i64));
            }
            if let Some(v) = ti(n, "interpolationdelay") {
                out.push(format!("start_interpolation:{v}"));
            }
            if let Some(v) = ti(n, "interpolationduration") {
                out.push(format!("interpolation_duration:{v}"));
            }
            for (key, nbt) in [("height", "height"), ("width", "width"), ("viewrange", "view_range")] {
                if let Some(v) = tnum(npc, n, key) {
                    out.push(format!("{nbt}:{}", float_nbt(v)));
                }
            }
            let (off, sc, l, r) = (tlist(npc, n, "offset", 3), tlist(npc, n, "scale", 3), tlist(npc, n, "leftrotation", 4), tlist(npc, n, "rightrotation", 4));
            if off.is_some() || sc.is_some() || l.is_some() || r.is_some() {
                let f = |v: Option<Vec<f64>>, d: &[f64]| format!("[{}]", v.unwrap_or_else(|| d.to_vec()).iter().map(|x| float_nbt(*x)).collect::<Vec<_>>().join(","));
                out.push(format!(
                    "transformation:{{translation:{},left_rotation:{},scale:{},right_rotation:{}}}",
                    f(off, &[0.0; 3]),
                    f(l, &[0.0, 0.0, 0.0, 1.0]),
                    f(sc, &[1.0; 3]),
                    f(r, &[0.0, 0.0, 0.0, 1.0])
                ));
            }
            if kind == "minecraft:item_display"
                && let Some(t) = ts("itemdisplay", "transform").filter(|t| {
                    ["none", "thirdperson_lefthand", "thirdperson_righthand", "firstperson_lefthand", "firstperson_righthand", "head", "gui", "ground", "fixed"].contains(&t.as_str())
                })
            {
                out.push(format!("item_display:\"{t}\""));
            }
        }
        _ => {}
    }
    // Modifiers of every mob type: `attribute` and `hitbox --scale` become attribute bases,
    // `potioneffect` the entity's active effects.
    if let Some(n) = npc.map.at(&["traits", "attribute"]) {
        for e in n.entries() {
            if let (Some(id), Some(v)) = (attribute_id(&e.key), e.value.as_f64().or_else(|| e.value.as_i64().map(|i| i as f64))) {
                attrs.push(format!("{{id:\"minecraft:{id}\",base:{}}}", dbl(v)));
            }
        }
    }
    // The general flags that show in the entity's data.
    let flag_of = |name: &str| tbool_key(npc, name, "enabled");
    if let Some(sit) = flag_of("sitting")
        && matches!(kind, "minecraft:wolf" | "minecraft:cat" | "minecraft:parrot")
        && !out.iter().any(|o| o.starts_with("Sitting:"))
    {
        out.push(format!("Sitting:{}b", b1(sit)));
    }
    if let Some(pick) = flag_of("pickupitems") {
        out.push(format!("CanPickUpLoot:{}b", b1(pick)));
    }
    if flag_of("knockback") == Some(false) && !attrs.iter().any(|a| a.contains("knockback_resistance")) {
        attrs.push(format!("{{id:\"minecraft:knockback_resistance\",base:{}}}", dbl(1.0)));
    }
    if let Some(v) = tnum(npc, "scaledmaxhealth", "value").filter(|v| *v > 0.0) {
        if !attrs.iter().any(|a| a.contains("minecraft:max_health")) {
            attrs.push(format!("{{id:\"minecraft:max_health\",base:{}}}", dbl(v)));
        }
        out.push(format!("Health:{}", float_nbt(v)));
    }
    if let Some(v) = tnum(npc, "hitbox", "scale") {
        attrs.retain(|a| !a.contains("minecraft:scale\""));
        attrs.push(format!("{{id:\"minecraft:scale\",base:{}}}", dbl(v)));
    }
    if !attrs.is_empty() {
        out.push(format!("attributes:[{}]", attrs.join(",")));
    }
    // (spawn.rs already writes the persistent invisibility effect; that one wins.)
    if npc.invisibility().is_none()
        && let Some(n) = npc.map.at(&["traits", "potioneffect"])
    {
        let effects: Vec<String> = n
            .entries()
            .iter()
            .filter_map(|e| {
                let id = ns_id(&e.value.get("type").and_then(Node::text)?.to_ascii_lowercase())?;
                let dur = e.value.get("duration").and_then(Node::as_i64).unwrap_or(600);
                let amp = e.value.get("amplifier").and_then(Node::as_i64).unwrap_or(0).clamp(0, 255);
                let icon = e.value.get("icon").and_then(Node::as_bool).unwrap_or(true);
                Some(format!("{{id:{id},amplifier:{amp}b,duration:{dur},show_particles:1b,show_icon:{}b,ambient:0b}}", b1(icon)))
            })
            .collect();
        if !effects.is_empty() {
            out.push(format!("active_effects:[{}]", effects.join(",")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_citizens_core::config::Config;
    use crate::horizon_citizens_core::entity_set::{EntitySet, Synched};

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
        let r = cmd(&mut w, &s, "npc wolf -s -a -i --collar red --variant ashen");
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

    fn nbt(w: &World, kind: &str) -> Vec<String> {
        live_nbt(w.saves.get(0).unwrap(), kind)
    }

    #[test]
    fn tameable_variants_reach_the_entity() {
        let (mut w, s) = setup("wolf");
        let r = cmd(&mut w, &s, "npc wolf -s -a --collar red --variant ashen");
        assert!(!r.iter().any(|l| l.contains("does not act")), "{r:?}");
        assert_eq!(nbt(&w, "minecraft:wolf"), ["Sitting:1b", "AngerTime:2147483647", "CollarColor:14b", "variant:\"minecraft:ashen\""]);
        let r = cmd(&mut w, &s, "npc wolf -i");
        assert!(r.iter().any(|l| l.contains("does not act")), "interested is synched only: {r:?}");
        let (mut w, s) = setup("cat");
        cmd(&mut w, &s, "npc cat -s --type siamese --ccolor blue");
        assert_eq!(nbt(&w, "minecraft:cat"), ["variant:\"minecraft:siamese\"", "CollarColor:11b", "Sitting:1b"]);
        let (mut w, s) = setup("parrot");
        cmd(&mut w, &s, "npc parrot --variant gray");
        assert_eq!(nbt(&w, "minecraft:parrot"), ["Variant:4"]);
    }

    #[test]
    fn horse_llama_rabbit_fox_and_friends() {
        let (mut w, s) = setup("horse");
        cmd(&mut w, &s, "npc horse --color black --style white_dots -t");
        assert_eq!(nbt(&w, "minecraft:horse"), [format!("Variant:{}", 4 | (3 << 8)), "Tame:1b".to_owned()]);
        let (mut w, s) = setup("llama");
        cmd(&mut w, &s, "npc llama --color gray --strength 4");
        assert_eq!(nbt(&w, "minecraft:llama"), ["Variant:3", "Strength:4"]);
        let (mut w, s) = setup("rabbit");
        cmd(&mut w, &s, "npc rabbittype the_killer_bunny");
        assert_eq!(nbt(&w, "minecraft:rabbit"), ["RabbitType:99"]);
        let (mut w, s) = setup("fox");
        cmd(&mut w, &s, "npc fox --type snow --sleeping true --crouching false");
        assert_eq!(nbt(&w, "minecraft:fox"), ["Type:\"snow\"", "Sleeping:1b", "Crouching:0b"]);
        let (mut w, s) = setup("tropical_fish");
        cmd(&mut w, &s, "npc tropicalfish --body red --pattern flopper --patterncolor blue");
        assert_eq!(nbt(&w, "minecraft:tropical_fish"), [format!("Variant:{}", 256 | (14 << 16) | (11 << 24))]);
        let (mut w, s) = setup("goat");
        cmd(&mut w, &s, "npc goat -n");
        assert_eq!(nbt(&w, "minecraft:goat"), ["HasLeftHorn:0b", "HasRightHorn:0b"]);
        let (mut w, s) = setup("bee");
        cmd(&mut w, &s, "npc bee -s --anger 40");
        assert_eq!(nbt(&w, "minecraft:bee"), ["HasStung:1b", "AngerTime:40"]);
        let (mut w, s) = setup("shulker");
        cmd(&mut w, &s, "npc shulker --peek 50 --color lime");
        assert_eq!(nbt(&w, "minecraft:shulker"), ["Peek:50b", "Color:5b"]);
    }

    #[test]
    fn display_frame_and_cloud_data() {
        let (mut w, s) = setup("item_display");
        cmd(&mut w, &s, "npc display --billboard center --brightness 15,7 --scale 2,2,2 --viewrange 3 --interpolationduration 20");
        cmd(&mut w, &s, "npc itemdisplay --transform gui");
        assert_eq!(
            nbt(&w, "minecraft:item_display"),
            [
                "billboard:\"center\"",
                "brightness:{block:15,sky:7}",
                "interpolation_duration:20",
                "view_range:3f",
                "transformation:{translation:[0f,0f,0f],left_rotation:[0f,0f,0f,1f],scale:[2f,2f,2f],right_rotation:[0f,0f,0f,1f]}",
                "item_display:\"gui\""
            ]
        );
        let (mut w, s) = setup("item_frame");
        cmd(&mut w, &s, "npc itemframe --visible false --fixed true --rotation 3 --face east");
        assert_eq!(nbt(&w, "minecraft:item_frame"), ["Invisible:1b", "Fixed:1b", "ItemRotation:3b", "Facing:5b"]);
        let (mut w, s) = setup("area_effect_cloud");
        cmd(&mut w, &s, "npc areaeffectcloud --radius 2.5 --duration 100 --radius_per_tick -0.5");
        assert_eq!(nbt(&w, "minecraft:area_effect_cloud"), ["Radius:2.5f", "Duration:100", "RadiusPerTick:-0.5f"]);
        let (mut w, s) = setup("minecart");
        cmd(&mut w, &s, "npc minecart --offset 9");
        assert_eq!(nbt(&w, "minecraft:minecart"), ["DisplayOffset:9"]);
    }

    #[test]
    fn attributes_hitbox_and_potions_reach_any_mob() {
        let (mut w, s) = setup("zombie");
        cmd(&mut w, &s, "npc attribute GENERIC_MAX_HEALTH 40");
        cmd(&mut w, &s, "npc hitbox --scale 2");
        cmd(&mut w, &s, "npc potioneffect add --type speed --duration 100 --amplifier 1 --icon false");
        assert_eq!(
            nbt(&w, "minecraft:zombie"),
            [
                "attributes:[{id:\"minecraft:max_health\",base:40.0d},{id:\"minecraft:scale\",base:2.0d}]",
                "active_effects:[{id:\"minecraft:speed\",amplifier:1b,duration:100,show_particles:1b,show_icon:0b,ambient:0b}]"
            ]
        );
    }

    #[test]
    fn mannequin_description_and_flags() {
        let (mut w, s) = setup("player");
        cmd(&mut w, &s, "npc mannequin --hide_description false --immovable false --main_hand left");
        let cfg = w.cfg.clone();
        let npc = w.saves.get(0).unwrap();
        let snbt = crate::horizon_citizens_core::spawn::npc_snbt(npc, 1, &npc.location().unwrap(), &cfg);
        assert!(snbt.contains("immovable:0b") && snbt.contains("hide_description:0b") && snbt.contains("main_hand:\"left\""), "{snbt}");
    }

    #[test]
    fn general_flags_show_in_the_entity_and_its_team() {
        let (mut w, s) = setup("wolf");
        cmd(&mut w, &s, "npc sitting true");
        cmd(&mut w, &s, "npc pickupitems true");
        cmd(&mut w, &s, "npc knockback false");
        assert_eq!(
            nbt(&w, "minecraft:wolf"),
            ["Sitting:1b", "CanPickUpLoot:1b", "attributes:[{id:\"minecraft:knockback_resistance\",base:1.0d}]"]
        );
        assert_eq!(w.saves.get(0).unwrap().team_key(), None);
        let r = cmd(&mut w, &s, "npc collidable false");
        assert!(!r.iter().any(|l| l.contains("does not act")), "{r:?}");
        assert_eq!(w.saves.get(0).unwrap().team_key().as_deref(), Some("nocollide"));
    }

    #[test]
    fn synched_only_states_say_so() {
        // The wolf's interested state has no host call.
        let (mut w, s) = setup("wolf");
        let r = cmd(&mut w, &s, "npc wolf -i");
        assert!(r.last().unwrap().contains("does not act on it"), "{r:?}");
        let (mut w, s) = setup("panda");
        let r = cmd(&mut w, &s, "npc panda -e");
        assert!(r.last().unwrap().contains("does not act on it"), "{r:?}");
    }

    fn entity_sets(w: &mut World, entity: u32) -> Vec<EntitySet> {
        w.take_ops().into_iter().filter_map(|o| if let Op::Entity { entity: e, set } = o { (e == entity).then_some(set) } else { None }).collect()
    }

    fn join(w: &mut World, entity: u32) {
        let uuid = w.saves.get(0).unwrap().uuid();
        assert!(!w.on_entity_join("minecraft:overworld", entity, uuid, true));
        w.take_ops();
    }

    #[test]
    fn synched_states_are_set_on_the_entity() {
        let cases: [(&str, &str, EntitySet); 8] = [
            ("allay", "npc allay -d", EntitySet::Synched(Synched::AllayDancing, true)),
            ("fox", "npc fox --interested true", EntitySet::Synched(Synched::FoxInterested, true)),
            ("fox", "npc fox --pouncing true", EntitySet::Synched(Synched::FoxPouncing, true)),
            ("fox", "npc fox --faceplanted false", EntitySet::Synched(Synched::FoxFaceplanted, false)),
            ("panda", "npc panda -n", EntitySet::Synched(Synched::PandaSneezing, true)),
            ("panda", "npc panda -r", EntitySet::Synched(Synched::PandaRolling, true)),
            ("polar_bear", "npc polarbear -r", EntitySet::Synched(Synched::PolarBearStanding, true)),
            ("vex", "npc vex --charging true", EntitySet::Synched(Synched::VexCharging, true)),
        ];
        for (kind, line, want) in cases {
            let (mut w, s) = setup(kind);
            join(&mut w, 31);
            let r = cmd(&mut w, &s, line);
            assert!(!r.iter().any(|l| l.contains("does not act")), "{line}: {r:?}");
            let got = entity_sets(&mut w, 31);
            assert!(got.contains(&want), "{line}: {got:?}");
        }
    }

    #[test]
    fn a_dragon_keeps_or_breaks_walls_on_the_entity() {
        let (mut w, s) = setup("ender_dragon");
        join(&mut w, 32);
        let r = cmd(&mut w, &s, "npc enderdragon --destroywalls false");
        assert!(!r.iter().any(|l| l.contains("does not act")), "{r:?}");
        assert!(entity_sets(&mut w, 32).contains(&EntitySet::DestroyBlocks(false)));
        // The setting is sent again when the entity comes back.
        let (mut w, s) = setup("ender_dragon");
        cmd(&mut w, &s, "npc enderdragon --destroywalls true");
        w.take_ops();
        join_keep(&mut w, 33);
        assert!(entity_sets(&mut w, 33).contains(&EntitySet::DestroyBlocks(true)));
    }

    fn join_keep(w: &mut World, entity: u32) {
        let uuid = w.saves.get(0).unwrap().uuid();
        assert!(!w.on_entity_join("minecraft:overworld", entity, uuid, true));
    }

    #[test]
    fn scaled_max_health_sets_health_and_attribute() {
        let (mut w, s) = setup("zombie");
        cmd(&mut w, &s, "npc scaledmaxhealth 50");
        assert_eq!(nbt(&w, "minecraft:zombie"), ["Health:50f", "attributes:[{id:\"minecraft:max_health\",base:50.0d}]"]);
    }
}
