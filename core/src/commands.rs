//! `/npc` and `/citizens`, following Citizens' `NPCCommands` and `AdminCommands`: the same
//! sub-commands, aliases, argument counts, permission nodes and message texts (from Citizens'
//! `en.json`), with its flags (`--type`, `--at`, `--range`, ...). Most work on the sender's
//! selected NPC (`/npc select`, or the last one made).
//!
//! Derived from Citizens (https://github.com/CitizensDev/Citizens2, Open Software License 3.0).
//! Changed from the original: ported to Rust for Horizon. Commands that need a trait this port
//! does not have are not registered (see GAPS.md).

use crate::tree::TreeExt;
use crate::yaml::Node;
use crate::npc::{CommandSpec, Hand, Location, Nameplate, NpcData, Skin};
use crate::saves::Saves;
use crate::text;
use crate::world::{DespawnReason, Sender, World};

// ---- messages (Citizens' en.json, `{n}` arguments, `[[x]]` highlights) ------------------------

const M_NO_PERM: &str = "You don''t have permission to execute that command.";
const M_MUST_SELECT: &str = "You must have an NPC selected to execute that command.";
const M_MUST_OWN: &str = "You must be the owner of this NPC to execute that command.";
const M_MUST_BE_INGAME: &str = "You must be ingame to use that command.";
const M_TOO_FEW: &str = "Too few arguments.";
const M_TOO_MANY: &str = "Too many arguments.";
const M_INVALID_NUMBER: &str = "That is not a valid number.";
const M_INVALID_LOCATION: &str = "Invalid location";
const M_WORLD_NOT_FOUND: &str = "World not found.";
const M_NPC_NOT_FOUND: &str = "No NPC could be found.";
const M_NO_NPC_WITH_ID: &str = "No NPC with the ID {0} exists.";
const M_INVALID_MOBTYPE: &str = "[[{0}]] is not a valid mob type.";
const M_LIST_INVALID_MOBTYPE: &str = "[[{0}]] is not a valid mobtype.";
const M_NAME_TOO_LONG: &str = "NPC names cannot be longer than [[{0}]] characters. The name has been shortened.";
const M_DISALLOWED_MOBTYPE: &str = "The NPC cannot be the mob type [[{0}]] for that command.";
const M_UNKNOWN_REGISTRY: &str = "Unknown NPC registry [[{0}]].";
const M_PAGE_MISSING: &str = "The page [[{0}]] does not exist.";
const M_ALREADY_SELECTED: &str = "You already have that NPC selected. ";
const M_DESELECTED: &str = "Deselected NPC.";
const M_SELECTION_PROMPT: &str = "There were multiple NPCs with the supplied name.<br>Please enter an id or number from the list below to select that NPC.";
const M_COPIED: &str = "[[{0}]] has been copied.";
const M_DESPAWNED: &str = "You despawned [[{0}]].";
const M_SPAWNED: &str = "You spawned [[{0}]]. ";
const M_ALREADY_SPAWNED: &str = "[[{0}]] is already spawned at another location. Use ''/npc tphere'' to teleport the NPC to your location.";
const M_NO_STORED_LOCATION: &str = "No stored location available - command must be used ingame. ";
const M_REMOVED: &str = "You permanently removed [[{0}]] ID [[{1}]] (<click:run_command:/npc undo><hover:show_text:Undo this removal><red><u>undo</u></red></hover></click>).";
const M_REMOVED_ALL: &str = "You permanently removed all NPCs.";
const M_NPCS_REMOVED: &str = "NPCs removed. ";
const M_RENAMED: &str = "You renamed [[{0}]] to [[{1}]].";
const M_TP_NO_LOCATION: &str = "Couldn''t find the target NPC''s location.";
const M_TELEPORTED_TO_NPC: &str = "You teleported to [[{0}]].";
const M_MISSING_CURSOR_BLOCK: &str = "Please look at a block to teleport to.";
// Citizens ships no text for this key: it shows the key itself.
const M_NO_MULTIWORLD: &str = "citizens.commands.npc.tphere.multiworld-not-allowed";
const M_NPC_TELEPORTED: &str = "[[{0}]] was teleported to {1}.";
const M_MOVETO_FORMAT: &str = "Format is x:y:z(:world) or x y z( world).";
const M_MOVETO_TELEPORTED: &str = "[[{0}]] teleported to [[{1}]].";
const M_MOVETO_NO_SPAWN: &str = "NPC could not be spawned.";
const M_GLOWING_SET: &str = "[[{0}]] is now glowing.";
const M_GLOWING_UNSET: &str = "[[{0}]] is no longer glowing.";
const M_GLOWING_COLOR_SET: &str = "[[{0}]]''s glowing color set to {1}]].";
const M_GLOWING_FORMAT: &str = "Color [[{0}]] must not be a formatting color.";
const M_GRAVITY_ENABLED: &str = "Gravity [[enabled]].";
const M_GRAVITY_DISABLED: &str = "Gravity [[disabled]].";
const M_LOOKCLOSE_SET: &str = "[[{0}]] will now rotate when players are nearby.";
const M_LOOKCLOSE_STOPPED: &str = "[[{0}]] will no longer rotate when players are nearby.";
const M_LOOKCLOSE_RANDOM_SET: &str = "[[{0}]] will now randomly look around.";
const M_LOOKCLOSE_RANDOM_STOPPED: &str = "[[{0}]] will no longer randomly look around.";
const M_LOOKCLOSE_RANGE_SET: &str = "[[{0}]]''s lookclose range set to [[{1}]] blocks.";
const M_LOOKCLOSE_PITCH_SET: &str = "[[{0}]] will now randomly choose a pitch in the range [{1}].";
const M_LOOKCLOSE_YAW_SET: &str = "[[{0}]] will now randomly choose a yaw in the range [{1}].";
const M_LOOKCLOSE_RANGE_ERROR: &str = "Invalid range [[{0}]]. Use the format `min,max`.";
const M_NAMEPLATE_SET: &str = "Nameplate visibility set to [[{0}]].";
const M_OWNER: &str = "[[{0}]]''s owner is [[{1}]].";
const M_ALREADY_OWNER: &str = "{0} is already the owner of [[{1}]].";
const M_OWNER_SET_SERVER: &str = "[[The server]] is now the owner of [[{0}]].";
const M_OWNER_SET: &str = "[[{1}]] is now the owner of [[{0}]].";
const M_VULNERABLE_SET: &str = "[[{0}]] is now vulnerable.";
const M_VULNERABLE_STOPPED: &str = "[[{0}]] is no longer vulnerable.";
const M_INVALID_ENTITY_TYPE: &str = "[[{0}]] is not a valid type.";
const M_ENTITY_TYPE_SET: &str = "[[{0}]]''s type set to [[{1}]].";
const M_SKIN_SET: &str = "[[{0}]]''s skin name set to [[{1}]].";
// Citizens sends this one without its argument, so the `{0}` stays.
const M_SKIN_CLEARED: &str = "[[{0}]]''s skin name was cleared.";
const M_SKIN_REQUIRED: &str = "A skin name is required.";
const M_SKIN_URL_ERROR: &str = "Error downloading skin texture from [[{0}]]. Are you sure the URL is valid?";
const M_EQUIPMENT_SET: &str = "Set [[{0}]] to [[{1}]].";
const M_HOLO_HEADER: &str = "[[{0}]]''s hologram text (in bottom-up order):";
const M_HOLO_INVALID_LINE: &str = "Invalid line number.";
const M_HOLO_TEXT_MISSING: &str = "Missing text to add.";
const M_HOLO_LINE_ADD: &str = "Added a new hologram line: [[{0}]].";
const M_HOLO_LINE_SET: &str = "Set hologram text line [[{0}]] to [[{1}]].";
const M_HOLO_LINE_REMOVED: &str = "Removed line [[{0}]].";
const M_HOLO_CLEARED: &str = "Hologram lines cleared.";
const M_HOLO_HEIGHT_SET: &str = "Line height set to [[{0}]].";
const M_HOLO_MARGIN_MISSING: &str = "Missing margin to set.";
const M_HOLO_MARGIN_SET: &str = "Set hologram line [[{0}]] margin [[{1}]] to [[{2}]].";
const M_CMD_ADDED: &str = "Command [[{0}]] added with id [[{1}]].";
const M_CMD_REMOVED: &str = "Command [[{0}]] removed.";
const M_CMD_CLEARED: &str = "[[{0}]]''s commands cleared.";
const M_CMD_UNKNOWN_ID: &str = "Unknown command id [[{0}]] for this NPC.";
const M_CMD_NONE: &str = "No commands have been added.";
const M_CMD_LEFT: &str = "Commands to run on [[left click]]:";
const M_CMD_RIGHT: &str = "Commands to run on [[right click]]:";
const M_CMD_DESCRIBE: &str = "<br>    - {0} [{1}s] [cost:{2}] [exp:{3}] [<click:run_command:/npc cmd remove {4}><hover:show_text:Remove this command><red><u>-</hover></click>]";
const M_CITIZENS_RELOAD_WARNING: &str = "WARNING: this command will load all data from disk without saving first. Please retype /citizens reload to confirm. You can disable this warning in settings.";
const M_CITIZENS_RELOADING: &str = "Reloading Citizens...";
const M_CITIZENS_RELOADED: &str = "Citizens reloaded.";
const M_CITIZENS_RELOAD_ERROR: &str = "Error occured while reloading, see console.";
const M_CITIZENS_SAVING: &str = "Saving Citizens... ";
const M_CITIZENS_SAVED: &str = "Citizens saved.";
/// Not in Citizens: something it can do that this port cannot (yet).
const M_UNSUPPORTED: &str = "[[{0}]] is not supported on Horizon yet.";
/// Not in Citizens: the text-line, equipment and path sub-commands are its chat/GUI editors; here
/// they are plain sub-commands.
const M_AGE_CANNOT: &str = "The mob type {0} cannot be aged.";
const M_AGE_INVALID: &str = "Invalid age. Valid ages are adult, baby, number between -24000 and 0";
const M_AGE_LOCKED: &str = "Age locked.";
const M_AGE_UNLOCKED: &str = "Age unlocked.";
const M_AGE_BABY: &str = "[[{0}]] is now a baby.";
const M_AGE_ADULT: &str = "[[{0}]] is now an adult.";
const M_AGE_NORMAL: &str = "[[{0}]] is now age [[{1}]].";
const M_AGE_DESCRIBE: &str = "{0}''s age is [[{1}]]. Locked is [[{2}]].";
const M_POWERED_SET: &str = "[[{0}]] will now be powered.";
const M_POWERED_STOPPED: &str = "[[{0}]] will no longer be powered.";
const M_SHEEP_COLOR_SET: &str = "The sheep''s color was set to [[{0}]].";
const M_SHEEP_INVALID_COLOR: &str = "Invalid sheep color given. Valid colors are: [[{0}]]. ";
const M_SIZE_DESCRIBE: &str = "[[{0}]]''s size is [[{1}]].";
const M_SIZE_SET: &str = "[[{0}]]''s size set to [[{1}]].";
const M_REQ_MOBTYPE: &str = "The NPC cannot be the mob type [[{0}]] for that command.";
const M_FOLLOW_SET: &str = "[[{0}]] is now following [[{1}]]. ";
const M_FOLLOW_UNSET: &str = "[[{0}]] is no longer following anyone.";
const M_FOLLOW_MARGIN: &str = "[[{0}]]''s will now follow [[{1}]] blocks away from the target.";
const M_SPEED_SET: &str = "NPC speed modifier set to [[{0}]].";
const M_TPTO_FROM: &str = "Source entity not found.";
const M_TPTO_TO: &str = "Destination entity not found.";
const M_TPTO_OK: &str = "Teleported successfully.";
const M_ANCHOR_ADDED: &str = "Anchor added.";
const M_ANCHOR_EXISTS: &str = "The anchor [[{0}]] already exists.";
const M_ANCHOR_INVALID: &str = "Invalid anchor name.";
// Citizens passes one argument for a message with `{1}`: the placeholder stays.
const M_ANCHOR_MISSING: &str = "The anchor [[{1}]] does not exist.";
const M_ANCHOR_REMOVED: &str = "Anchor removed.";
const M_POSE_ADDED: &str = "Pose added.";
const M_POSE_EXISTS: &str = "The pose [[{0}]] already exists.";
const M_POSE_DEFAULT: &str = "Default pose set to [[{0}]].";
const M_POSE_INVALID: &str = "Invalid pose name.";
const M_POSE_MISSING: &str = "The pose [[{0}]] does not exist.";
const M_POSE_REMOVED: &str = "Pose removed.";
const M_HURT_PROTECTED: &str = "NPC is protected and cannot be hurt. <click:run_command:/npc vulnerable>[[<u>Click to unprotect</click>";
const M_HURT_NOT_DAMAGEABLE: &str = "NPC type [[{0}]] cannot be hurt.";
const M_PROVIDER_SET: &str = "Set the waypoint provider to [[{0}]].";
const M_WANDER_DELAY: &str = "Delay set to [[{0}]] ticks.";
const M_WANDER_PATHFIND: &str = "Wander pathfinding set to [[{0}]].";
const M_WANDER_RANGE: &str = "Wander range set to [[{0}]] blocks (x/z) and [[{1}]] blocks (y).";
const M_SOUND_INFO: &str = "[[{0}]]''s sounds are: ambient - [[{1}]] hurt - [[{2}]] and death - [[{3}]].<br><br>Valid sounds are {4}.";
const M_SOUND_SET: &str = "[[{0}]]''s sounds are now: ambient - [[{1}]] hurt - [[{2}]] and death - [[{3}]].";
const M_NO_SUCH_LINE: &str = "No such line.";
const M_TRAITS_ADDED: &str = "Added {0} successfully.";
const M_TRAITS_FAILED_TO_ADD: &str = "Couldn''t add {0}.";
const M_TRAITS_REMOVED: &str = "Removed {0} successfully.";
const M_TRAITS_FAILED_TO_REMOVE: &str = "Couldn''t remove {0}.";
const M_FLYABLE_SET: &str = "[[{0}]] is now flyable.";
const M_FLYABLE_UNSET: &str = "[[{0}]] is no longer flyable.";
const M_GAMEMODE_DESCRIBE: &str = "{0}''s gamemode is [[{1}]].";
const M_GAMEMODE_INVALID: &str = "{0} is not a valid gamemode.";
const M_GAMEMODE_SET: &str = "Gamemode set to [[{0}]].";
const M_REQUIREMENTS_INVALID_MOB_TYPE: &str = "Mob type [[{0}]] is not valid for this command.";

// ---- the command table --------------------------------------------------------------------

/// One `/npc` sub-command: its name and aliases (`modifiers`), the usage text Citizens shows, the
/// argument count limits (the modifier itself counts as one) and its permission node.
struct Cmd {
    name: &'static str,
    aliases: &'static [&'static str],
    usage: &'static str,
    min: usize,
    max: Option<usize>,
    node: &'static str,
}

const fn cmd(
    name: &'static str,
    aliases: &'static [&'static str],
    usage: &'static str,
    min: usize,
    max: Option<usize>,
    node: &'static str,
) -> Cmd {
    Cmd { name, aliases, usage, min, max, node }
}

const CMDS: &[Cmd] = &[
    cmd("help", &[], "help", 1, None, "citizens.npc.help"),
    // Not a Citizens modifier (its bare `/npc` shows the info); accepted as the same thing.
    cmd("info", &[], "info", 1, Some(1), "citizens.npc.info"),
    cmd("debug", &[], "debug -p(aths) -n(avigation) -i(tem in hand)", 1, Some(1), "citizens.npc.debug"),
    cmd("gamemode", &[], "gamemode [gamemode]", 1, Some(2), "citizens.npc.gamemode"),
    cmd("flyable", &[], "flyable (true|false)", 1, Some(2), "citizens.npc.flyable"),
    cmd("copy", &[], "copy (--name newname)", 1, Some(1), "citizens.npc.copy"),
    cmd(
        "create",
        &[],
        "create [name] ((-b(aby),u(nspawned),s(ilent),t(emporary),c(enter),p(acket)) --at [x,y,z,world] --type [type] --item (item) --trait ['trait1, trait2...'] --model [model name] --nameplate [true|false|hover] --temporaryduration [duration] --registry [registry name]",
        2,
        None,
        "citizens.npc.create",
    ),
    cmd("age", &[], "age [age] (-l(ock))", 1, Some(2), "citizens.npc.age"),
    cmd("powered", &[], "powered (--set true|false)", 1, Some(1), "citizens.npc.powered"),
    cmd("sheep", &[], "sheep (--color [color]) (--sheared [sheared])", 1, Some(1), "citizens.npc.sheep"),
    cmd("slimesize", &[], "slimesize [size]", 1, Some(2), "citizens.npc.slimesize"),
    cmd("follow", &[], "follow (player name|NPC id) (-p[rotect]) (--margin [margin]) (--enable [boolean])", 1, Some(2), "citizens.npc.follow"),
    cmd("speed", &[], "speed [speed]", 2, Some(2), "citizens.npc.speed"),
    cmd("pathto", &[], "pathto me | here | cursor | [x] [y] [z] (--margin [distance margin]) (-s[traight line])", 2, Some(4), "citizens.npc.pathto"),
    cmd("rotate", &[], "rotate (--towards [x,y,z]) (--toentity [name|uuid|me]) (--body [yaw]) (--head [yaw]) (--pitch [pitch]) (-s(mooth))", 1, Some(1), "citizens.npc.rotate"),
    cmd("tpto", &[], "tpto [player name|npc id] [player name|npc id]", 2, Some(3), "citizens.npc.tpto"),
    cmd("anchor", &[], "anchor (--save [name]|--assume [name]|--remove [name]) (-a) (-c)", 1, Some(3), "citizens.npc.anchor"),
    cmd("pose", &[], "pose (--save [name] (-d) | --mirror [name] (-d) | --assume [name] | --remove [name] | --default [name]) (--yaw yaw) (--pitch pitch) (-a)", 1, Some(2), "citizens.npc.pose"),
    cmd("sound", &[], "sound (--death [death sound|d]) (--ambient [ambient sound|d]) (--hurt [hurt sound|d]) (-n(one)/-s(ilent)) (-d(efault))", 1, Some(1), "citizens.npc.sound"),
    cmd("hurt", &[], "hurt [damage]", 2, Some(2), "citizens.npc.hurt"),
    cmd("playsound", &[], "playsound [sound] (category) (volume) (pitch) (--to [player]) (--at x:y:z:world)", 2, Some(5), "citizens.npc.playsound"),
    cmd("wander", &[], "wander (add x y z world) | (worldguardregion [region]) | (xyrange [xrange] [yrange])", 1, Some(6), "citizens.npc.wander"),
    cmd("deselect", &["desel"], "deselect", 1, Some(1), "citizens.npc.deselect"),
    cmd("despawn", &[], "despawn (id)", 1, Some(2), "citizens.npc.despawn"),
    cmd("glowing", &[], "glowing --color [minecraft chat color]", 1, Some(1), "citizens.npc.glowing"),
    cmd("gravity", &[], "gravity", 1, Some(1), "citizens.npc.gravity"),
    cmd(
        "hologram",
        &[],
        "hologram add [text] (--duration [duration]) | insert [line #] [text] | set [line #] [text] | remove [line #] | edit_npc [template | name | line #] | clear | updaterate [ticks] | lineheight [height] | verticalviewrange [range] | viewrange [range] | margintop [line #] [margin] | marginbottom [line #] [margin] | bgcolor [line #] [color]",
        1,
        None,
        "citizens.npc.hologram",
    ),
    cmd("id", &[], "id", 1, Some(1), "citizens.npc.id"),
    cmd(
        "list",
        &[],
        "list (page) (-a[ll]) --owner [owner] --type [type] --registry [name] --tag [tag] --filter [filter expression]",
        1,
        Some(2),
        "citizens.npc.list",
    ),
    cmd(
        "lookclose",
        &["look"],
        "lookclose --range [range] --filter [filter] -r[ealistic looking] --randomlook [true|false] --perplayer [true|false] --randomswitchtargets [true|false] --randompitchrange [min,max] --randomyawrange [min,max] --disablewhennavigating [true|false] --targetnpcs [true|false]",
        1,
        Some(1),
        "citizens.npc.lookclose",
    ),
    cmd("moveto", &[], "moveto x:y:z:world | x y z world", 1, None, "citizens.npc.moveto"),
    cmd("name", &["hidename"], "name (-h(over))", 1, Some(1), "citizens.npc.name"),
    cmd("owner", &[], "owner [uuid|SERVER]", 1, Some(2), "citizens.npc.owner"),
    cmd(
        "remove",
        &["rem"],
        "remove|rem (all|id|name| --owner [owner] | --eid [entity uuid] | --world [world])",
        1,
        Some(2),
        "citizens.npc.remove",
    ),
    cmd("rename", &[], "rename [name]", 2, None, "citizens.npc.rename"),
    cmd(
        "select",
        &["sel"],
        "select|sel [id|name] (--range range) (--registry [name])",
        1,
        Some(2),
        "citizens.npc.select",
    ),
    cmd("setequipment", &[], "setequipment (-c(osmetic)) [slot] [item]", 2, Some(3), "citizens.npc.setequipment"),
    cmd(
        "skin",
        &[],
        "skin (-e(xport) -c(lear) -l(atest) -s(kull) -b(edrock) -u(rl)) [name] (or --url [url] --file [file] (-s(lim)) or -t [uuid/name] [data] [signature])",
        1,
        Some(4),
        "citizens.npc.skin",
    ),
    cmd("spawn", &[], "spawn (id|name) -l(oad chunks)", 1, Some(2), "citizens.npc.spawn"),
    cmd("tp", &["teleport"], "tp (-e(xact))", 1, Some(1), "citizens.npc.tp"),
    cmd(
        "tphere",
        &["tph", "move"],
        "tphere (cursor) -c(enter) -f(ront)",
        1,
        Some(2),
        "citizens.npc.tphere",
    ),
    cmd("type", &[], "type [type]", 2, Some(2), "citizens.npc.type"),
    cmd("vulnerable", &[], "vulnerable (-t(emporary))", 1, Some(1), "citizens.npc.vulnerable"),
    cmd(
        "command",
        &["cmd"],
        "command (add [command] | remove [id|all]) (-s(hift)) (-l[eft]/-r[ight]) (-p[layer] -o[p]), --cooldown --gcooldown [seconds] --delay [ticks] --permissions [perms] --n [max # of uses] --gn [max # of global uses]",
        1,
        None,
        "citizens.npc.command",
    ),
    // Citizens opens an editor for these three; here they are sub-commands.
    cmd("text", &[], "text [list|add <text>|remove <line>|clear]", 1, None, "citizens.npc.edit.text"),
    cmd(
        "equip",
        &[],
        "equip <hand|off_hand|helmet|chestplate|leggings|boots> [item] [amount]",
        1,
        None,
        "citizens.npc.edit.equip",
    ),
    cmd("path", &[], "path [list|add|remove <point>|clear]", 1, None, "citizens.npc.edit.path"),
];

/// Citizens modifiers this port does not have (they answer "not supported", not "Too many arguments").
const CITIZENS_ONLY: &[&str] = &["activationrange", "aggressive", "ai", "armorstand", "attribute", "bat", "behavior", "breakblock", "chunkload", "cload", "collidable", "pushable", "configgui", "controllable", "control", "disguise", "drops", "endercrystal", "enderman", "entitypose", "eval", "fish", "forcefield", "hitbox", "home", "horse", "donkey", "mule", "inventory", "item", "itemframe", "jump", "knockback", "leashable", "metadata", "minecart", "mirror", "mount", "nophysics", "ocelot", "packet", "painting", "passive", "pathopt", "po", "patho", "pausepathfinding", "pickupitems", "panimate", "playerfilter", "playerlist", "rabbittype", "rbtype", "respawn", "rotationsettings", "scaledmaxhealth", "shop", "shopitem", "showshop", "sitting", "skinlayers", "speak", "spectate", "swim", "target", "targetable", "trackingrange", "undo", "useitem", "velocity", "vel", "wither", "wolf"];

fn find_cmd(sub: &str) -> Option<&'static Cmd> {
    let sub = sub.to_ascii_lowercase();
    CMDS.iter().find(|c| c.name == sub || c.aliases.contains(&sub.as_str()))
}

const COLOURS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];

/// Usage tails per sub-command for the game's tab completion (`<name:kind>` as Horizon's
/// `NativeCommand` takes them). Aliases get the same tails. Free text is always last.
const TAILS: &[(&str, &[&str])] = &[
    ("help", &[""]),
    ("info", &[""]),
    ("debug", &["", "-p", "-n", "-i"]),
    ("gamemode", &["", "survival", "creative", "adventure", "spectator"]),
    ("flyable", &["", "true", "false"]),
    ("copy", &["", "--name <name:text>"]),
    (
        "create",
        &[
            "<name:text>",
            "--type <type:word> <name:text>",
            "--at <at:word> <name:text>",
            "--nameplate true <name:text>",
            "--nameplate false <name:text>",
            "--nameplate hover <name:text>",
            "-u <name:text>",
            "-s <name:text>",
            "-c <name:text>",
            "-u -c <name:text>",
        ],
    ),
    ("age", &["", "baby", "adult", "-l", "<age:integer>"]),
    ("powered", &["", "--set true", "--set false"]),
    ("sheep", &["--sheared true", "--sheared false", "--color <color:word>"]),
    ("slimesize", &["", "<size:integer>"]),
    ("follow", &["", "<target:word>", "-p", "--margin <margin:number>", "--enable true", "--enable false"]),
    ("speed", &["<speed:number>"]),
    ("pathto", &["me", "here", "<x:number> <y:number> <z:number>", "<x:number> <y:number> <z:number> --margin <margin:number>"]),
    (
        "rotate",
        &[
            "--towards <x,y,z:word>",
            "--toentity me",
            "--toentity <entity:word>",
            "--body <yaw:number>",
            "--head <yaw:number>",
            "--pitch <pitch:number>",
        ],
    ),
    ("tpto", &["<from:word>", "<from:word> <to:word>"]),
    ("anchor", &["", "--save <name:word>", "--assume <name:word>", "--remove <name:word>", "-a"]),
    (
        "pose",
        &[
            "",
            "--save <name:word>",
            "--save <name:word> -d",
            "--mirror <name:word>",
            "--assume <name:word>",
            "--remove <name:word>",
            "--default <name:word>",
            "-a",
        ],
    ),
    ("sound", &["", "--ambient <sound:word>", "--hurt <sound:word>", "--death <sound:word>", "-n", "-s", "-d"]),
    ("hurt", &["<damage:integer>"]),
    ("playsound", &["<sound:word>", "<sound:word> <category:word> <volume:number> <pitch:number>", "<sound:word> --to <player:word>"]),
    ("wander", &["", "xyrange <xrange:integer> <yrange:integer>", "delay <ticks:integer>", "pathfind true", "pathfind false"]),
    ("deselect", &[""]),
    ("despawn", &["", "<target:word>"]),
    ("glowing", &["", "--color <color:word>"]),
    ("gravity", &[""]),
    (
        "hologram",
        &[
            "",
            "add <text:text>",
            "insert <line:word> <text:text>",
            "set <line:word> <text:text>",
            "remove <line:word>",
            "clear",
            "lineheight <height:number>",
            "margintop <line:word> <margin:number>",
            "marginbottom <line:word> <margin:number>",
        ],
    ),
    ("id", &[""]),
    (
        "list",
        &["", "<page:integer>", "-a", "--owner <owner:word>", "--type <type:word>", "--page <page:integer>"],
    ),
    (
        "lookclose",
        &[
            "",
            "--range <range:number>",
            "--randomlook true",
            "--randomlook false",
            "--randompitchrange <range:word>",
            "--randomyawrange <range:word>",
        ],
    ),
    (
        "moveto",
        &[
            "<x:number> <y:number> <z:number>",
            "<x:number> <y:number> <z:number> <world:word>",
            "<position:word>",
        ],
    ),
    ("name", &["", "-h"]),
    ("owner", &["", "server", "<owner:word>"]),
    (
        "remove",
        &["", "all", "<target:word>", "--owner <owner:word>", "--eid <entity:word>", "--world <world:word>"],
    ),
    ("rename", &["<name:text>"]),
    ("select", &["", "<target:word>", "--range <range:number>", "--registry <name:word>"]),
    (
        "setequipment",
        &[
            "hand [item:item]",
            "off_hand [item:item]",
            "head [item:item]",
            "chest [item:item]",
            "legs [item:item]",
            "feet [item:item]",
        ],
    ),
    ("skin", &["", "-c", "-t <uuid:word> <data:word> <signature:word>", "<name:word>"]),
    ("spawn", &["", "<target:word>", "-l", "<target:word> -l"]),
    ("tp", &["", "-e"]),
    ("tphere", &["", "cursor", "-c", "-f", "-c -f"]),
    ("type", &["<type:word>"]),
    ("vulnerable", &["", "-t"]),
    (
        "command",
        &[
            "",
            "add <command:text>",
            "add -l <command:text>",
            "add -r <command:text>",
            "add -l -r <command:text>",
            "add -s <command:text>",
            "add -p <command:text>",
            "add -o <command:text>",
            "add -p -o <command:text>",
            "add -l -p <command:text>",
            "add -r -p <command:text>",
            "add -l -p -o <command:text>",
            "add -r -p -o <command:text>",
            "add --cooldown <seconds:number> <command:text>",
            "add --delay <ticks:integer> <command:text>",
            "remove all",
            "remove <id:word>",
        ],
    ),
    ("text", &["", "list", "add <line:text>", "remove <line:integer>", "clear"]),
    (
        "equip",
        &[
            "hand [item:item] [amount:integer]",
            "off_hand [item:item] [amount:integer]",
            "helmet [item:item] [amount:integer]",
            "chestplate [item:item] [amount:integer]",
            "leggings [item:item] [amount:integer]",
            "boots [item:item] [amount:integer]",
        ],
    ),
    ("path", &["", "list", "add", "remove <point:integer>", "clear"]),
];

/// Every usage registered with the game, so chat completes them. Horizon merges them into one
/// tree per root.
pub fn usages() -> Vec<String> {
    // Horizon takes a command's permission from its first usage; Citizens' `plugin.yml` gives
    // `/npc` `citizens.npc.help` and `/citizens` `citizens.citizens.help`.
    let mut out = Vec::new();
    for (name, tails) in TAILS {
        let Some(c) = find_cmd(name) else { continue };
        for n in std::iter::once(c.name).chain(c.aliases.iter().copied()) {
            for tail in tails.iter() {
                if *name == "glowing" && tail.starts_with("--color") {
                    for colour in COLOURS {
                        out.push(format!("npc {n} --color {colour}"));
                    }
                }
                out.push(format!("npc {n} {tail}").trim().to_owned());
            }
        }
    }
    out.push("npc".to_owned());
    for u in ["trait add <names:text>", "trait remove <names:text>", "trait list", "trait <names:text>"] {
        out.push(u.to_owned());
    }
    for u in ["citizens help", "citizens", "citizens save", "citizens reload", "citizens load"] {
        out.push(u.to_owned());
    }
    out
}

/// The help lines (`/npc help`): the syntax as players type it, without argument kinds.
pub fn help_lines() -> Vec<String> {
    CMDS.iter().map(|c| format!("npc {}", c.usage)).collect()
}

/// Every permission node a command asks, as Citizens' `plugin.yml` names them (plus the ones its
/// code checks without listing them: `citizens.npc.remove`, `citizens.npc.admin`).
pub const NODES: &[&str] = &[
    "citizens.admin",
    "citizens.npc.debug",
    "citizens.npc.gamemode",
    "citizens.npc.flyable",
    "citizens.npc.trait",
    "citizens.admin.remove.all",
    "citizens.citizens.help",
    "citizens.npc.admin",
    "citizens.npc.age",
    "citizens.npc.follow",
    "citizens.npc.speed",
    "citizens.npc.pathto",
    "citizens.npc.rotate",
    "citizens.npc.tpto",
    "citizens.npc.anchor",
    "citizens.npc.pose",
    "citizens.npc.hurt",
    "citizens.npc.sound",
    "citizens.npc.playsound",
    "citizens.npc.wander",
    "citizens.npc.follow.others",
    "citizens.npc.powered",
    "citizens.npc.sheep",
    "citizens.npc.slimesize",
    "citizens.npc.create.*",
    "citizens.npc.createall",
    "citizens.npc.create.player",
    "citizens.npc.copy",
    "citizens.npc.create",
    "citizens.npc.deselect",
    "citizens.npc.despawn",
    "citizens.npc.edit.equip",
    "citizens.npc.edit.path",
    "citizens.npc.edit.text",
    "citizens.npc.glowing",
    "citizens.npc.gravity",
    "citizens.npc.help",
    "citizens.npc.hologram",
    "citizens.npc.id",
    "citizens.npc.info",
    "citizens.npc.list",
    "citizens.npc.lookclose",
    "citizens.npc.moveto",
    "citizens.npc.name",
    "citizens.npc.owner",
    "citizens.npc.remove",
    "citizens.npc.rename",
    "citizens.npc.select",
    "citizens.npc.setequipment",
    "citizens.npc.skin",
    "citizens.npc.spawn",
    "citizens.npc.tp",
    "citizens.npc.tphere",
    "citizens.npc.tphere.multiworld",
    "citizens.npc.type",
    "citizens.npc.vulnerable",
    "citizens.npc.command",
];

/// The permission node of a command, by its root word and its first word after it. A word that
/// is no sub-command asks for `citizens.npc.info`, as Citizens' bare `/npc` does.
pub fn node_for(root: &str, sub: &str) -> &'static str {
    if root == "citizens" {
        return match sub.to_ascii_lowercase().as_str() {
            "help" => "citizens.citizens.help",
            _ => "citizens.admin",
        };
    }
    if root == "trait" {
        return "citizens.npc.trait";
    }
    if sub.is_empty() {
        return "citizens.npc.info";
    }
    find_cmd(sub).map_or("citizens.npc.info", |c| c.node)
}

/// The permission node of a registered usage (`npc create <name:text>`).
pub fn node_of_usage(usage: &str) -> &'static str {
    let mut words = usage.split_whitespace();
    let root = words.next().unwrap_or("");
    let sub = words.next().unwrap_or("");
    node_for(root, sub)
}

/// Citizens' `plugin.yml` `children:` edges (parent, child) that lead to a node in [`NODES`], wildcards
/// included, copied from the 2.0.44-b4268 jar. A parent granted to a player grants every child under it.
const PARENT_EDGES: &[(&str, &str)] = &[
    ("citizens.npc.trait.*", "citizens.npc.trait"),
    ("citizens.admin.*", "citizens.admin"),
    ("citizens.admin.*", "citizens.admin.remove.all"),
    ("citizens.npc.*", "citizens.npc.age"),
    ("citizens.npc.*", "citizens.npc.anchor"),
    ("citizens.npc.*", "citizens.npc.command"),
    ("citizens.npc.*", "citizens.npc.copy"),
    ("citizens.npc.*", "citizens.npc.create"),
    ("citizens.npc.*", "citizens.npc.create.player"),
    ("citizens.npc.*", "citizens.npc.createall"),
    ("citizens.npc.*", "citizens.npc.debug"),
    ("citizens.npc.*", "citizens.npc.deselect"),
    ("citizens.npc.*", "citizens.npc.despawn"),
    ("citizens.npc.*", "citizens.npc.edit.equip"),
    ("citizens.npc.*", "citizens.npc.edit.path"),
    ("citizens.npc.*", "citizens.npc.edit.text"),
    ("citizens.npc.*", "citizens.npc.flyable"),
    ("citizens.npc.*", "citizens.npc.follow"),
    ("citizens.npc.*", "citizens.npc.follow.others"),
    ("citizens.npc.*", "citizens.npc.gamemode"),
    ("citizens.npc.*", "citizens.npc.glowing"),
    ("citizens.npc.*", "citizens.npc.gravity"),
    ("citizens.npc.*", "citizens.npc.help"),
    ("citizens.npc.*", "citizens.npc.hologram"),
    ("citizens.npc.*", "citizens.npc.hurt"),
    ("citizens.npc.*", "citizens.npc.id"),
    ("citizens.npc.*", "citizens.npc.info"),
    ("citizens.npc.*", "citizens.npc.list"),
    ("citizens.npc.*", "citizens.npc.lookclose"),
    ("citizens.npc.*", "citizens.npc.moveto"),
    ("citizens.npc.*", "citizens.npc.name"),
    ("citizens.npc.*", "citizens.npc.owner"),
    ("citizens.npc.*", "citizens.npc.pathto"),
    ("citizens.npc.*", "citizens.npc.playsound"),
    ("citizens.npc.*", "citizens.npc.pose"),
    ("citizens.npc.*", "citizens.npc.powered"),
    ("citizens.npc.*", "citizens.npc.rename"),
    ("citizens.npc.*", "citizens.npc.rotate"),
    ("citizens.npc.*", "citizens.npc.select"),
    ("citizens.npc.*", "citizens.npc.setequipment"),
    ("citizens.npc.*", "citizens.npc.sheep"),
    ("citizens.npc.*", "citizens.npc.skin"),
    ("citizens.npc.*", "citizens.npc.slimesize"),
    ("citizens.npc.*", "citizens.npc.sound"),
    ("citizens.npc.*", "citizens.npc.spawn"),
    ("citizens.npc.*", "citizens.npc.speed"),
    ("citizens.npc.*", "citizens.npc.tp"),
    ("citizens.npc.*", "citizens.npc.tphere"),
    ("citizens.npc.*", "citizens.npc.tphere.multiworld"),
    ("citizens.npc.*", "citizens.npc.tpto"),
    ("citizens.npc.*", "citizens.npc.trait"),
    ("citizens.npc.*", "citizens.npc.type"),
    ("citizens.npc.*", "citizens.npc.vulnerable"),
    ("citizens.npc.*", "citizens.npc.wander"),
    ("citizens.*", "citizens.admin"),
    ("citizens.*", "citizens.admin.remove.all"),
    ("citizens.*", "citizens.citizens.help"),
    ("citizens.*", "citizens.npc.age"),
    ("citizens.*", "citizens.npc.anchor"),
    ("citizens.*", "citizens.npc.command"),
    ("citizens.*", "citizens.npc.copy"),
    ("citizens.*", "citizens.npc.create"),
    ("citizens.*", "citizens.npc.create.player"),
    ("citizens.*", "citizens.npc.createall"),
    ("citizens.*", "citizens.npc.debug"),
    ("citizens.*", "citizens.npc.deselect"),
    ("citizens.*", "citizens.npc.despawn"),
    ("citizens.*", "citizens.npc.edit.equip"),
    ("citizens.*", "citizens.npc.edit.path"),
    ("citizens.*", "citizens.npc.edit.text"),
    ("citizens.*", "citizens.npc.flyable"),
    ("citizens.*", "citizens.npc.follow"),
    ("citizens.*", "citizens.npc.follow.others"),
    ("citizens.*", "citizens.npc.gamemode"),
    ("citizens.*", "citizens.npc.glowing"),
    ("citizens.*", "citizens.npc.gravity"),
    ("citizens.*", "citizens.npc.help"),
    ("citizens.*", "citizens.npc.hologram"),
    ("citizens.*", "citizens.npc.hurt"),
    ("citizens.*", "citizens.npc.id"),
    ("citizens.*", "citizens.npc.info"),
    ("citizens.*", "citizens.npc.list"),
    ("citizens.*", "citizens.npc.lookclose"),
    ("citizens.*", "citizens.npc.moveto"),
    ("citizens.*", "citizens.npc.name"),
    ("citizens.*", "citizens.npc.owner"),
    ("citizens.*", "citizens.npc.pathto"),
    ("citizens.*", "citizens.npc.playsound"),
    ("citizens.*", "citizens.npc.pose"),
    ("citizens.*", "citizens.npc.powered"),
    ("citizens.*", "citizens.npc.rename"),
    ("citizens.*", "citizens.npc.rotate"),
    ("citizens.*", "citizens.npc.select"),
    ("citizens.*", "citizens.npc.setequipment"),
    ("citizens.*", "citizens.npc.sheep"),
    ("citizens.*", "citizens.npc.skin"),
    ("citizens.*", "citizens.npc.slimesize"),
    ("citizens.*", "citizens.npc.sound"),
    ("citizens.*", "citizens.npc.spawn"),
    ("citizens.*", "citizens.npc.speed"),
    ("citizens.*", "citizens.npc.tp"),
    ("citizens.*", "citizens.npc.tphere"),
    ("citizens.*", "citizens.npc.tphere.multiworld"),
    ("citizens.*", "citizens.npc.tpto"),
    ("citizens.*", "citizens.npc.trait"),
    ("citizens.*", "citizens.npc.type"),
    ("citizens.*", "citizens.npc.vulnerable"),
    ("citizens.*", "citizens.npc.wander"),
    ("citizens.npc.command.*", "citizens.npc.command"),
    ("citizens.npc.tphere.*", "citizens.npc.tphere"),
    ("citizens.npc.tphere.*", "citizens.npc.tphere.multiworld"),
    ("citizens.npc.edit.*", "citizens.npc.edit.equip"),
    ("citizens.npc.edit.*", "citizens.npc.edit.path"),
    ("citizens.npc.edit.*", "citizens.npc.edit.text"),
    ("citizens.npc.create.*", "citizens.npc.create"),
    ("citizens.npc.create.*", "citizens.npc.create.player"),
    ("citizens.npc.follow.*", "citizens.npc.follow"),
    ("citizens.npc.follow.*", "citizens.npc.follow.others"),
];

/// The nodes above `node` in the tree (`citizens.npc.skin` has `citizens.npc.*` and `citizens.*`), nearest
/// first in the order they are found. A player holding one of them holds `node` (`PARENT_EDGES`).
pub fn parents_of(node: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut todo: Vec<&str> = vec![node];
    while let Some(current) = todo.pop() {
        for &(parent, child) in PARENT_EDGES {
            if child == current && !out.contains(&parent) {
                out.push(parent);
                todo.push(parent);
            }
        }
    }
    out
}

/// Whether a sender may use `node`. Operators of `command_level` and above always may, as before;
/// others need the node, or `citizens.admin`, granted by the permissions hook.
pub fn allowed(sender: &Sender, command_level: u8, node: &str) -> bool {
    sender.permission >= command_level || sender.grants.iter().any(|g| g == node || g == "citizens.admin")
}

/// Like [`allowed`] but for a node `citizens.admin` does not stand in for.
fn exact(sender: &Sender, command_level: u8, node: &str) -> bool {
    sender.permission >= command_level || sender.grants.iter().any(|g| g == node)
}

// ---- argument parsing ---------------------------------------------------------------------

/// Splits arguments into words and `--flag value` / `-f` flags, as Citizens does. The words are
/// what follows the sub-command.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub words: Vec<String>,
    pub values: Vec<(String, String)>,
    pub switches: Vec<char>,
}

impl Args {
    pub fn parse(s: &str) -> Args {
        let mut a = Args::default();
        let tokens: Vec<&str> = s.split_whitespace().collect();
        let mut i = 0;
        while i < tokens.len() {
            let t = tokens[i];
            if let Some(name) = t.strip_prefix("--")
                && !name.is_empty()
            {
                if let Some((k, v)) = name.split_once('=') {
                    a.values.push((k.to_ascii_lowercase(), v.to_owned()));
                } else {
                    let v = tokens.get(i + 1).copied().unwrap_or("");
                    a.values.push((name.to_ascii_lowercase(), v.to_owned()));
                    i += 1;
                }
            } else if let Some(sw) = t.strip_prefix('-')
                && !sw.is_empty()
                && sw.chars().all(|c| c.is_ascii_alphabetic())
            {
                a.switches.extend(sw.chars().map(|c| c.to_ascii_lowercase()));
            } else {
                a.words.push(t.to_owned());
            }
            i += 1;
        }
        a
    }
    pub fn value(&self, k: &str) -> Option<&str> {
        self.values.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn has_value(&self, k: &str) -> bool {
        self.values.iter().any(|(n, _)| n == k)
    }
    pub fn has(&self, c: char) -> bool {
        self.switches.contains(&c)
    }
    pub fn rest(&self, from: usize) -> String {
        self.words.get(from..).map(|w| w.join(" ")).unwrap_or_default()
    }
}

// ---- rendering ------------------------------------------------------------------------------

/// A command's answer: lines for the sender (legacy colour codes).
type Reply = Vec<String>;
type Res = Result<Reply, Reply>;

/// The legacy colour code of a colour name (`dark_red` -> `4`).
fn code_of(name: &str) -> Option<char> {
    "0123456789abcdef".chars().find(|c| text::color_name(*c) == Some(name))
}

/// Citizens' MiniMessage-ish tags as legacy codes: `<yellow>`, `<u>`, `<red>`..., with the end
/// tags back to `base`. Click and hover tags are dropped; any other `<...>` stays text.
fn tags(s: &str, base: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find('>') else {
            out.push('<');
            rest = after;
            continue;
        };
        let t = after[..j].to_ascii_lowercase();
        let mut consumed = true;
        if t.starts_with("click:") || t.starts_with("hover:") || t == "/click" || t == "/hover" {
            // dropped
        } else if t == "u" {
            out.push_str("&n");
        } else if t == "/u" {
            out.push_str("&r");
            out.push_str(base);
        } else if let Some(c) = code_of(&t) {
            out.push('&');
            out.push(c);
        } else if t.strip_prefix('/').is_some_and(|n| code_of(n).is_some()) {
            out.push_str(base);
        } else {
            consumed = false;
        }
        if consumed {
            rest = &after[j + 1..];
        } else {
            out.push('<');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `[[` starts the highlight colour, `]]` goes back to `base`.
fn highlights(s: &str, base: &str, highlight: &str) -> String {
    let mut out = String::from(base);
    let mut rest = s;
    loop {
        let open = rest.find("[[");
        let close = rest.find("]]");
        match (open, close) {
            (Some(o), c) if c.is_none_or(|c| o < c) => {
                out.push_str(&rest[..o]);
                out.push_str(highlight);
                rest = &rest[o + 2..];
            }
            (_, Some(c)) => {
                out.push_str(&rest[..c]);
                out.push_str(base);
                rest = &rest[c + 2..];
            }
            _ => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

/// A message as chat lines: tags, highlights, `<br>` line breaks.
fn render(w: &World, error: bool, s: &str) -> Reply {
    let m = &w.cfg.messages;
    let base = if error { &m.error } else { &m.message };
    s.split("<br>")
        .map(|line| highlights(&tags(line, base), base, &m.highlight))
        .collect()
}

/// Fills `{n}` in a message and takes `''` as the single quote it stands for.
fn fill(t: &str, args: &[&str]) -> String {
    let mut s = t.replace("''", "'");
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

fn ok(w: &World, t: &str, args: &[&str]) -> Reply {
    render(w, false, &fill(t, args))
}

fn err(w: &World, t: &str, args: &[&str]) -> Reply {
    render(w, true, &fill(t, args))
}

/// A line that is already filled in.
fn say(w: &World, s: &str) -> Reply {
    render(w, false, s)
}

fn header(s: &str) -> String {
    format!("<yellow>=====[</yellow> {s}<yellow> ]=====")
}

/// The usage error of the command framework: the problem, then the syntax.
fn usage_error(w: &World, problem: Option<&str>, c: &Cmd) -> Reply {
    let mut out = Vec::new();
    if let Some(p) = problem {
        out.extend(err(w, p, &[]));
    }
    out.push(w.cfg.messages.error(&format!("/npc {}", c.usage)));
    out
}

fn usage_of(w: &World, name: &str) -> Reply {
    match find_cmd(name) {
        Some(c) => usage_error(w, None, c),
        None => Vec::new(),
    }
}

/// Java's `Double.toString` for the numbers a command prints back.
fn jnum(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e7 { format!("{v:.1}") } else { v.to_string() }
}

/// Citizens' `#.##` decimal format.
fn dec2(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    if s.is_empty() || s == "-0" { "0".into() } else { s }
}

/// `Util.prettyPrintLocation`.
fn pretty_location(l: &Location) -> String {
    format!(
        "{} at {}, {}, {} ({}, {})",
        l.world,
        dec2(l.x),
        dec2(l.y),
        dec2(l.z),
        dec2(f64::from(l.yaw)),
        dec2(f64::from(l.pitch))
    )
}

/// `Util.prettyEnum`: `SNOW_GOLEM` -> `snow golem`.
fn pretty_enum(s: &str) -> String {
    s.to_ascii_lowercase().replace('_', " ")
}

/// An NPC's name as Citizens' `getName()` gives it: without colour codes.
fn name_of(npc: &NpcData) -> String {
    text::strip(&npc.name())
}

fn name_at(w: &World, id: i32) -> String {
    w.saves.get(id).map(name_of).unwrap_or_default()
}

/// The NPC `id` to change; "no NPC" when it has gone.
fn npc_mut(w: &mut World, id: i32) -> Result<&mut NpcData, Reply> {
    if w.saves.get(id).is_none() {
        return Err(err(w, M_NPC_NOT_FOUND, &[]));
    }
    w.saves.get_mut(id).ok_or_else(Vec::new)
}

// ---- who is asking, what they selected ------------------------------------------------------

/// The console selects under a key no player UUID can have.
fn skey(sender: &Sender) -> u128 {
    sender.uuid.unwrap_or(u128::MAX)
}

fn is_admin(w: &World, sender: &Sender) -> bool {
    allowed(sender, w.cfg.command_level, "citizens.admin")
}

/// Citizens' `Owner.isOwnedBy(sender)`: the owner, the console, or an administrator.
fn owns(w: &World, sender: &Sender, id: i32) -> bool {
    sender.uuid.is_none()
        || is_admin(w, sender)
        || w.saves.get(id).and_then(NpcData::owner).is_some_and(|o| Some(o) == sender.uuid)
}

fn selected_id(w: &World, sender: &Sender) -> Result<i32, Reply> {
    w.selected(skey(sender)).ok_or_else(|| err(w, M_MUST_SELECT, &[]))
}

/// The selected NPC, which the sender must own (`@Requirements(selected = true, ownership = true)`).
fn target(w: &World, sender: &Sender) -> Result<i32, Reply> {
    let id = selected_id(w, sender)?;
    if !owns(w, sender, id) {
        return Err(err(w, M_MUST_OWN, &[]));
    }
    Ok(id)
}

fn here(w: &World, sender: &Sender) -> Result<Location, Reply> {
    sender
        .at
        .as_ref()
        .map(|(_, l)| l.clone())
        .ok_or_else(|| err(w, M_MUST_BE_INGAME, &[]))
}

/// `x:y:z[:world]` (or with commas) as `--at` takes it.
fn parse_at(w: &World, s: &str, sender: &Sender) -> Option<Location> {
    let parts: Vec<&str> = s.split([':', ',']).collect();
    if parts.len() < 3 {
        return None;
    }
    let world = parts
        .get(3)
        .map(|s| (*s).to_owned())
        .or_else(|| sender.at.as_ref().map(|(_, l)| l.world.clone()))
        .unwrap_or_else(|| w.cfg.worlds.main.clone());
    let yaw: f32 = parts.get(4).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    Some(Location {
        world,
        x: parts[0].parse().ok()?,
        y: parts[1].parse().ok()?,
        z: parts[2].parse().ok()?,
        yaw,
        pitch: parts.get(5).and_then(|v| v.parse().ok()).unwrap_or(0.0),
        body_yaw: yaw,
    })
}

/// What `NPCCommandSelector.startWithCallback` finds for an id, UUID or name.
enum Found {
    One(i32),
    /// Several NPCs of that name.
    Many(Vec<i32>),
    /// An id or UUID no NPC has: Citizens' callback gets `null`.
    Missing,
    /// A name no NPC has: Citizens says nothing.
    Silent,
}

fn find(w: &World, raw: &str, range: Option<f64>, sender: &Sender) -> Found {
    if raw.len() == 36
        && let Some(u) = crate::ids::parse_uuid(raw)
    {
        return w.saves.by_uuid(u).map_or(Found::Missing, |n| Found::One(n.id));
    }
    if let Ok(id) = raw.parse::<i32>() {
        return if w.saves.get(id).is_some() { Found::One(id) } else { Found::Missing };
    }
    let mut possible: Vec<i32> = Vec::new();
    let mut npcs: Vec<&NpcData> = w.saves.npcs.iter().collect();
    npcs.sort_by_key(|n| n.id);
    for n in npcs {
        if !name_of(n).eq_ignore_ascii_case(raw) {
            continue;
        }
        if let (Some(r), Some((_, at))) = (range, sender.at.as_ref())
            && w.is_out(n.id)
            && let Some(l) = w.current_location(n.id)
            && crate::look::dist2([l.x, l.y, l.z], [at.x, at.y, at.z]) > r * r
        {
            continue;
        }
        possible.push(n.id);
    }
    match possible.len() {
        0 => Found::Silent,
        1 => Found::One(possible[0]),
        _ => Found::Many(possible),
    }
}

/// The prompt Citizens' conversation shows for several NPCs of one name. Answering it in chat is
/// not possible here: `/npc select <id>` is the way.
fn selection_prompt(w: &World, ids: &[i32]) -> Reply {
    let mut s = fill(M_SELECTION_PROMPT, &[]);
    for id in ids {
        s.push_str(&format!("<br>    - {id}"));
    }
    render(w, false, &s)
}

// ---- entry ----------------------------------------------------------------------------------

/// Runs `/npc ...` or `/citizens ...` (the line without its slash). `reload` is passed for
/// `/citizens reload`, read by the glue from the file.
pub fn run(w: &mut World, sender: &Sender, line: &str, reload: impl FnOnce() -> Result<Saves, String>) -> Reply {
    let line = line.trim().trim_start_matches('/');
    let (root, rest) = line.split_once(' ').unwrap_or((line, ""));
    let sub = rest.split_whitespace().next().unwrap_or("");
    // `remove` checks its own permission (`citizens.npc.remove` or `citizens.admin`) inside.
    let own_check = root == "npc" && find_cmd(sub).is_some_and(|c| c.name == "remove");
    if !own_check && !allowed(sender, w.cfg.command_level, node_for(root, sub)) {
        return err(w, M_NO_PERM, &[]);
    }
    match root {
        "citizens" => citizens(w, sender, rest, reload),
        "trait" => match trait_cmd(w, sender, rest) {
            Ok(r) | Err(r) => r,
        },
        _ => match npc(w, sender, rest) {
            Ok(r) | Err(r) => r,
        },
    }
}

fn citizens(w: &mut World, sender: &Sender, rest: &str, reload: impl FnOnce() -> Result<Saves, String>) -> Reply {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let sub = words.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    match sub.as_str() {
        "" => {
            let mut out = say(w, &header(&format!("<green>Citizens v{}", env!("CARGO_PKG_VERSION"))));
            out.extend(say(w, "     <yellow>-- <green>Author: fullwall"));
            out.extend(say(w, "     <yellow>-- <green><u>Website</u> <u>Support</u>"));
            out
        }
        "reload" | "load" if words.len() == 1 => {
            let key = skey(sender);
            let confirmed = w.reload_warned.get(&key).is_some_and(|until| w.tick <= *until);
            if !confirmed {
                // Five seconds to type it again.
                let until = w.tick + 100;
                w.reload_warned.insert(key, until);
                return err(w, M_CITIZENS_RELOAD_WARNING, &[]);
            }
            let mut out = ok(w, M_CITIZENS_RELOADING, &[]);
            match reload() {
                Ok(saves) => {
                    let ids: Vec<i32> = w.saves.npcs.iter().map(|n| n.id).collect();
                    for id in ids {
                        w.despawn(id, DespawnReason::Command);
                    }
                    w.saves = saves;
                    out.extend(ok(w, M_CITIZENS_RELOADED, &[]));
                }
                Err(e) => {
                    eprintln!("[citizens] reload failed: {e}");
                    out.extend(err(w, M_CITIZENS_RELOAD_ERROR, &[]));
                }
            }
            out
        }
        "save" if words.len() == 1 => {
            let mut out = ok(w, M_CITIZENS_SAVING, &[]);
            match w.save_now() {
                Ok(()) => out.extend(ok(w, M_CITIZENS_SAVED, &[])),
                Err(e) => {
                    eprintln!("[citizens] save failed: {e}");
                    out.extend(err(w, "Could not save, see console.", &[]));
                }
            }
            out
        }
        "help" => {
            let mut out = say(w, &header("Help"));
            for u in ["citizens", "citizens reload", "citizens save"] {
                out.extend(say(w, &format!("/{u}")));
            }
            out
        }
        _ => {
            let mut out = err(w, M_TOO_MANY, &[]);
            out.push(w.cfg.messages.error("/citizens"));
            out
        }
    }
}

fn npc(w: &mut World, sender: &Sender, rest: &str) -> Res {
    let (sub, tail) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
    if sub.is_empty() {
        return info(w, sender);
    }
    let Some(c) = find_cmd(sub) else {
        // A real Citizens command this port lacks: say so instead of "Too many arguments".
        if CITIZENS_ONLY.contains(&sub.to_ascii_lowercase().as_str()) {
            return Err(err(w, M_UNSUPPORTED, &[&format!("npc {}", sub.to_ascii_lowercase())]));
        }
        // Not a modifier: Citizens falls back to the bare `/npc`, which takes no arguments.
        let bare = Cmd { name: "", aliases: &[], usage: "", min: 0, max: Some(0), node: "citizens.npc.info" };
        return Err(usage_error(w, Some(M_TOO_MANY), &bare));
    };
    let args = Args::parse(tail);
    // The modifier counts as an argument, as it does for Citizens.
    let count = 1 + args.words.len();
    if count < c.min {
        return Err(usage_error(w, Some(M_TOO_FEW), c));
    }
    if c.max.is_some_and(|max| count > max) {
        return Err(usage_error(w, Some(M_TOO_MANY), c));
    }
    match c.name {
        "help" => Ok(help(w)),
        "info" => info(w, sender),
        "debug" => debug(w, sender, &args),
        "gamemode" => gamemode(w, sender, &args),
        "flyable" => flyable(w, sender, &args),
        "copy" => copy(w, sender, &args),
        "create" => create(w, sender, &args),
        "age" => age(w, sender, &args),
        "follow" => follow(w, sender, &args),
        "speed" => speed(w, sender, &args),
        "pathto" => pathto(w, sender, &args),
        "rotate" => rotate(w, sender, &args),
        "tpto" => tpto(w, sender, &args),
        "anchor" => anchor(w, sender, &args),
        "pose" => pose(w, sender, &args),
        "hurt" => hurt(w, sender, &args),
        "sound" => sound(w, sender, &args),
        "playsound" => playsound(w, sender, &args),
        "wander" => wander(w, sender, &args),
        "powered" => powered(w, sender, &args),
        "sheep" => sheep(w, sender, &args),
        "slimesize" => slimesize(w, sender, &args),
        "deselect" => {
            w.deselect(skey(sender));
            Ok(ok(w, M_DESELECTED, &[]))
        }
        "despawn" => despawn(w, sender, &args),
        "glowing" => glowing(w, sender, &args),
        "gravity" => gravity(w, sender),
        "hologram" => hologram(w, sender, tail),
        "id" => target(w, sender).map(|id| vec![id.to_string()]),
        "list" => list(w, sender, &args),
        "lookclose" => lookclose(w, sender, &args),
        "moveto" => moveto(w, sender, &args),
        "name" => nameplate(w, sender, &args),
        "owner" => owner_cmd(w, sender, &args),
        "remove" => remove(w, sender, &args),
        "rename" => rename(w, sender, &args),
        "select" => select(w, sender, &args),
        "setequipment" => setequipment(w, sender, &args, c),
        "skin" => skin(w, sender, &args),
        "spawn" => spawn(w, sender, &args),
        "tp" => tp(w, sender, &args),
        "tphere" => tphere(w, sender, &args),
        "type" => set_type(w, sender, &args),
        "vulnerable" => vulnerable(w, sender),
        "command" => command(w, sender, tail),
        "text" => text_lines(w, sender, tail),
        "equip" => equip(w, sender, &args),
        "path" => path(w, sender, tail),
        _ => Err(usage_error(w, Some(M_TOO_MANY), c)),
    }
}

fn help(w: &World) -> Reply {
    let mut out = say(w, &header("Help"));
    for l in help_lines() {
        out.extend(say(w, &format!("/{l}")));
    }
    out
}

fn info(w: &World, sender: &Sender) -> Res {
    let id = target(w, sender)?;
    let npc = w.saves.get(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
    let mut out = say(w, &header(&name_of(npc)));
    out.extend(say(w, &format!("    ID: [[{id}]]")));
    out.extend(say(w, &format!("    UUID: [[{}]]", npc.uuid().map(crate::ids::format_uuid).unwrap_or_default())));
    out.extend(say(w, &format!("    Type: [[{}]]", npc.kind())));
    if w.is_out(id)
        && let Some(l) = w.current_location(id)
    {
        out.extend(say(
            w,
            &format!(
                "    Spawned at [[{}, {}, {}, {:.2}, {:.2} (head {:.2})]] [[{}]]",
                l.x.floor() as i64,
                l.y.floor() as i64,
                l.z.floor() as i64,
                l.yaw,
                l.pitch,
                l.yaw,
                l.world
            ),
        ));
    }
    out.extend(say(w, "    Traits"));
    for t in npc.trait_names() {
        out.extend(say(w, &format!("     - [[{t}]]")));
    }
    Ok(out)
}

// ---- creating, copying, removing, selecting -------------------------------------------------

/// Bukkit's longest NPC name: 16 for a player, else 256. (Guess: that code is not in Citizens'
/// plugin source.)
fn max_name_length(kind: &str) -> usize {
    if kind.eq_ignore_ascii_case("PLAYER") { 16 } else { 256 }
}

/// The entity types Citizens accepts for `--type` and `npc type`, in lower case: the names its
/// `citizens.npc.create.<type>` nodes list in plugin.yml (Citizens 2.0.44), plus the older Bukkit names
/// that `spawn::entity_kind` maps. Any other word is refused with the invalid-type line.
const ENTITY_TYPES: &[&str] = &[
    "acacia_boat", "acacia_chest_boat", "allay", "area_effect_cloud", "armadillo", "armor_stand", "arrow",
    "axolotl", "bamboo_chest_raft", "bamboo_raft", "bat", "bee", "birch_boat", "birch_chest_boat", "blaze",
    "block_display", "bogged", "breeze", "breeze_wind_charge", "camel", "camel_husk", "cat", "cave_spider",
    "cherry_boat", "cherry_chest_boat", "chest_minecart", "chicken", "cod", "command_block_minecart",
    "copper_golem", "cow", "creaking", "creeper", "dark_oak_boat", "dark_oak_chest_boat", "dolphin", "donkey",
    "dragon_fireball", "drowned", "egg", "elder_guardian", "end_crystal", "ender_dragon", "ender_pearl",
    "enderman", "endermite", "evoker", "evoker_fangs", "experience_bottle", "experience_orb", "eye_of_ender",
    "falling_block", "fireball", "firework_rocket", "fishing_bobber", "fox", "frog", "furnace_minecart",
    "ghast", "giant", "glow_item_frame", "glow_squid", "goat", "guardian", "happy_ghast", "hoglin",
    "hopper_minecart", "horse", "husk", "illusioner", "interaction", "iron_golem", "item", "item_display",
    "item_frame", "jungle_boat", "jungle_chest_boat", "leash_knot", "lightning_bolt", "lingering_potion",
    "llama", "llama_spit", "magma_cube", "mangrove_boat", "mangrove_chest_boat", "mannequin", "marker",
    "minecart", "mooshroom", "mule", "nautilus", "oak_boat", "oak_chest_boat", "ocelot",
    "ominous_item_spawner", "painting", "pale_oak_boat", "pale_oak_chest_boat", "panda", "parched", "parrot",
    "phantom", "pig", "piglin", "piglin_brute", "pillager", "player", "polar_bear", "pufferfish", "rabbit",
    "ravager", "salmon", "sheep", "shulker", "shulker_bullet", "silverfish", "skeleton", "skeleton_horse",
    "slime", "small_fireball", "sniffer", "snow_golem", "snowball", "spawner_minecart", "spectral_arrow",
    "spider", "splash_potion", "spruce_boat", "spruce_chest_boat", "squid", "stray", "strider", "tadpole",
    "text_display", "tnt", "tnt_minecart", "trader_llama", "trident", "tropical_fish", "turtle", "vex",
    "villager", "vindicator", "wandering_trader", "warden", "wind_charge", "witch", "wither",
    "wither_skeleton", "wither_skull", "wolf", "zoglin", "zombie", "zombie_horse", "zombie_nautilus",
    "zombie_villager", "zombified_piglin",
    // Older Bukkit names (`spawn::entity_kind` maps them).
    "snowman", "mushroom_cow", "pig_zombie", "villager_golem",
];

/// Whether `t` names an entity type, in any case (Bukkit's `EntityType` names, see [`ENTITY_TYPES`]).
fn valid_type(t: &str) -> bool {
    ENTITY_TYPES.contains(&t.to_ascii_lowercase().as_str())
}

/// Cuts a name to Citizens' length limit, telling the sender; the length counts without colours.
fn limit_name(w: &World, name: String, kind: &str, out: &mut Reply) -> String {
    let max = max_name_length(kind);
    if text::strip(&name).chars().count() > max {
        let mut problem = err(w, M_NAME_TOO_LONG, &[&max.to_string()]);
        out.append(&mut problem);
        return name.chars().take(max).collect();
    }
    name
}

fn create(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let level = w.cfg.command_level;
    let raw_type = args.value("type");
    let kind = raw_type.map_or_else(|| "PLAYER".to_owned(), str::to_ascii_uppercase);
    if let Some(t) = raw_type
        && !valid_type(&kind)
    {
        return Err(err(w, M_INVALID_MOBTYPE, &[t]));
    }
    let mut out = Vec::new();
    let name = limit_name(w, args.rest(0), &kind, &mut out);
    if name.is_empty() {
        return Err(out);
    }
    let lower = kind.to_ascii_lowercase();
    if !(exact(sender, level, "citizens.npc.create.*")
        || exact(sender, level, "citizens.npc.createall")
        || exact(sender, level, &format!("citizens.npc.create.{lower}")))
    {
        return Err(err(w, M_NO_PERM, &[]));
    }
    if (args.has_value("at") || args.has_value("registry") || args.has_value("trait") || args.has_value("template"))
        && !exact(sender, level, "citizens.npc.admin")
    {
        return Err(err(w, M_NO_PERM, &[]));
    }
    for (flag, what) in [('b', "-b"), ('p', "-p"), ('t', "-t")] {
        if args.has(flag) {
            return Err(err(w, M_UNSUPPORTED, &[what]));
        }
    }
    for what in ["item", "model", "registry", "temporaryduration", "template"] {
        if args.has_value(what) {
            return Err(err(w, M_UNSUPPORTED, &[&format!("--{what}")]));
        }
    }
    let mut loc = match args.value("at") {
        Some(at) => parse_at(w, at, sender).ok_or_else(|| err(w, M_INVALID_LOCATION, &[]))?,
        None => here(w, sender)?,
    };
    if w.cfg.worlds.dimension(&loc.world).is_none() {
        return Err(err(w, M_WORLD_NOT_FOUND, &[]));
    }
    if args.has('c') {
        loc.x = loc.x.floor() + 0.5;
        loc.y = loc.y.floor();
        loc.z = loc.z.floor() + 0.5;
    }
    let nameplate = args.value("nameplate").map(|v| match v.to_ascii_lowercase().as_str() {
        "hover" => Nameplate::Hover,
        "true" => Nameplate::Shown,
        _ => Nameplate::Hidden,
    });
    let silent = args.has('s');
    let spawned = !args.has('u');
    let mut wanted: Vec<String> = Vec::new();
    if let Some(list) = args.value("trait") {
        for t in list.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()) {
            if KNOWN_TRAITS.contains(&t.as_str()) {
                wanted.push(t);
            } else {
                out.extend(err(w, M_TRAITS_FAILED_TO_ADD, &[&format!("{t}: Trait not found")]));
            }
        }
    }
    let id = w.create_with(&name, &kind, loc, sender.uuid, spawned, |npc| {
        if silent {
            npc.map.put(&["metadata", "silent"], Node::bool(true));
        }
        if let Some(n) = nameplate {
            npc.set_nameplate(n);
        }
        for t in &wanted {
            npc.add_trait_name(t);
        }
    });
    w.select(skey(sender), id);
    let shown = name_at(w, id);
    out.extend(ok(w, "Created [[{0}]] (ID [[{1}]]).", &[&shown, &id.to_string()]));
    Ok(out)
}

fn copy(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let old = name_at(w, id);
    let raw_name = w.saves.get(id).map(NpcData::name).unwrap_or_default();
    let new_name = args.value("name").map_or_else(|| raw_name.clone(), str::to_owned);
    let loc = match &sender.at {
        Some((_, l)) => l.clone(),
        None => w.current_location(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?,
    };
    let new_id = w.copy(id, loc).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
    if new_name != raw_name {
        npc_mut(w, new_id)?.set_name(&new_name);
        w.respawn(new_id);
    }
    w.select(skey(sender), new_id);
    Ok(ok(w, M_COPIED, &[&old]))
}

fn remove(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let level = w.cfg.command_level;
    // `--owner` and `--world` are limited to what the sender owns.
    let owned_where = |w: &World, keep: &dyn Fn(&NpcData) -> bool| -> Vec<i32> {
        let mut ids: Vec<i32> = w
            .saves
            .npcs
            .iter()
            .filter(|n| owns(w, sender, n.id) && keep(n))
            .map(|n| n.id)
            .collect();
        ids.sort_unstable();
        ids
    };
    if let Some(owner) = args.value("owner") {
        let uuid = crate::ids::parse_uuid(owner);
        let ids = owned_where(w, &|n| uuid.is_some() && n.owner() == uuid);
        for id in ids {
            w.remove(id);
        }
        return Ok(ok(w, M_NPCS_REMOVED, &[]));
    }
    if let Some(world) = args.value("world") {
        let world = world.to_owned();
        let ids = owned_where(w, &|n| n.location().is_some_and(|l| l.world.eq_ignore_ascii_case(&world)));
        for id in ids {
            w.remove(id);
        }
        return Ok(ok(w, M_NPCS_REMOVED, &[]));
    }
    if let Some(eid) = args.value("eid") {
        // The NPC's entity carries the NPC's UUID.
        let hit = crate::ids::parse_uuid(eid).and_then(|u| w.saves.by_uuid(u)).map(|n| n.id);
        return match hit {
            Some(id) if owns(w, sender, id) => {
                let name = name_at(w, id);
                w.remove(id);
                Ok(ok(w, M_REMOVED, &[&name, &id.to_string()]))
            }
            _ => Err(err(w, M_NPC_NOT_FOUND, &[])),
        };
    }
    let can_remove = allowed(sender, level, "citizens.npc.remove");
    let remove_one = |w: &mut World, id: i32| -> Res {
        if !owns(w, sender, id) {
            return Err(err(w, M_MUST_OWN, &[]));
        }
        if !can_remove {
            return Err(err(w, M_NO_PERM, &[]));
        }
        let name = name_at(w, id);
        w.remove(id);
        Ok(ok(w, M_REMOVED, &[&name, &id.to_string()]))
    };
    match args.words.first() {
        None => {
            let id = selected_id(w, sender)?;
            remove_one(w, id)
        }
        Some(a) if a.eq_ignore_ascii_case("all") => {
            if !allowed(sender, level, "citizens.admin.remove.all") {
                return Err(err(w, M_NO_PERM, &[]));
            }
            let ids: Vec<i32> = w.saves.npcs.iter().map(|n| n.id).collect();
            for id in ids {
                w.remove(id);
            }
            Ok(ok(w, M_REMOVED_ALL, &[]))
        }
        Some(raw) => {
            let raw = raw.clone();
            match find(w, &raw, None, sender) {
                Found::One(id) => remove_one(w, id),
                Found::Missing => Err(err(w, M_MUST_SELECT, &[])),
                Found::Silent => Ok(Vec::new()),
                Found::Many(ids) => Ok(selection_prompt(w, &ids)),
            }
        }
    }
}

fn select(w: &mut World, sender: &Sender, args: &Args) -> Res {
    if let Some(r) = args.value("registry") {
        return Err(err(w, M_UNKNOWN_REGISTRY, &[r]));
    }
    let range = match args.value("range") {
        Some(r) => Some(r.parse::<f64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?.abs()),
        None => None,
    };
    let key = skey(sender);
    let current = w.selected(key);
    let chosen = match args.words.first() {
        None => {
            let (dim, at) = sender.at.clone().ok_or_else(|| err(w, M_MUST_BE_INGAME, &[]))?;
            nearest(w, &dim, &at, range.unwrap_or(15.0))
        }
        Some(raw) => match find(w, raw, range, sender) {
            Found::One(id) => Some(id),
            Found::Missing => None,
            Found::Silent => return Ok(Vec::new()),
            Found::Many(ids) => return Ok(selection_prompt(w, &ids)),
        },
    };
    let Some(id) = chosen else {
        return Err(err(w, M_NPC_NOT_FOUND, &[]));
    };
    if current == Some(id) {
        return Err(err(w, M_ALREADY_SELECTED, &[]));
    }
    w.select(key, id);
    let name = name_at(w, id);
    let line = w
        .cfg
        .selection
        .message
        .replace("<npc>", &name)
        .replace("<id>", &id.to_string());
    if line.is_empty() {
        return Ok(Vec::new());
    }
    Ok(say(w, &line))
}

/// The NPC nearest to `at` within `range`, in `dimension`.
pub fn nearest(w: &World, dimension: &str, at: &Location, range: f64) -> Option<i32> {
    w.saves
        .npcs
        .iter()
        .filter_map(|n| {
            let l = w.current_location(n.id)?;
            (w.is_out(n.id) && w.cfg.worlds.dimension(&l.world).as_deref() == Some(dimension)).then(|| {
                let d = crate::look::dist2([l.x, l.y, l.z], [at.x, at.y, at.z]);
                (n.id, d)
            })
        })
        .filter(|(_, d)| *d <= range * range)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

fn list(w: &World, sender: &Sender, args: &Args) -> Res {
    if let Some(r) = args.value("registry") {
        return Err(err(w, M_UNKNOWN_REGISTRY, &[r]));
    }
    for what in ["tag", "filter"] {
        if args.has_value(what) {
            return Err(err(w, M_UNSUPPORTED, &[&format!("--{what}")]));
        }
    }
    let mut npcs: Vec<&NpcData> = w.saves.npcs.iter().collect();
    npcs.sort_by_key(|n| n.id);
    let owner = args.value("owner").map(crate::ids::parse_uuid);
    let mut shown: Vec<&NpcData> = if args.has('a') {
        npcs
    } else if let Some(o) = owner {
        npcs.into_iter().filter(|n| o.is_some() && n.owner() == o).collect()
    } else if sender.uuid.is_some() {
        npcs.into_iter().filter(|n| n.owner() == sender.uuid).collect()
    } else {
        Vec::new()
    };
    if let Some(t) = args.value("type") {
        let kind = t.to_ascii_uppercase();
        if !valid_type(&kind) {
            return Err(err(w, M_LIST_INVALID_MOBTYPE, &[t]));
        }
        shown.retain(|n| n.kind().eq_ignore_ascii_case(&kind));
    }
    let page = match args.value("page").or_else(|| args.words.first().map(String::as_str)) {
        Some(p) => p.parse::<i64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?,
        None => 1,
    };
    // Citizens' Paginator is not part of the plugin's own source; nine lines to a page is a guess.
    const PER_PAGE: usize = 9;
    let pages = shown.len().div_ceil(PER_PAGE) as i64;
    if page < 1 || page > pages {
        return Err(err(w, M_PAGE_MISSING, &[&page.to_string()]));
    }
    let mut out = say(w, &header(&format!("NPCs {page}/{pages}")));
    for n in shown.iter().skip((page as usize - 1) * PER_PAGE).take(PER_PAGE) {
        out.extend(say(
            w,
            &format!("<yellow>{}</yellow> {} ([[tp]]) ([[summon]]) (<red>-</red>)", n.id, name_of(n)),
        ));
    }
    Ok(out)
}

// ---- spawning, moving -----------------------------------------------------------------------

fn despawn(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = match args.words.first() {
        Some(raw) => match find(w, raw, None, sender) {
            Found::One(id) => id,
            Found::Missing => return Err(err(w, M_NO_NPC_WITH_ID, &[raw])),
            Found::Silent => return Ok(Vec::new()),
            Found::Many(ids) => return Ok(selection_prompt(w, &ids)),
        },
        None => selected_id(w, sender)?,
    };
    if !owns(w, sender, id) {
        return Err(err(w, M_MUST_OWN, &[]));
    }
    let name = name_at(w, id);
    npc_mut(w, id)?.set_spawned(false);
    w.mark_dirty();
    w.despawn(id, DespawnReason::Command);
    Ok(ok(w, M_DESPAWNED, &[&name]))
}

fn spawn(w: &mut World, sender: &Sender, args: &Args) -> Res {
    // `@Requirements(ownership = true)`: a selected NPC must be the sender's.
    if let Some(sel) = w.selected(skey(sender))
        && !owns(w, sender, sel)
    {
        return Err(err(w, M_MUST_OWN, &[]));
    }
    let id = match args.words.first() {
        Some(raw) => match find(w, raw, None, sender) {
            Found::One(id) => id,
            Found::Missing => return Err(err(w, M_NO_NPC_WITH_ID, &[raw])),
            Found::Silent => return Ok(Vec::new()),
            Found::Many(ids) => return Ok(selection_prompt(w, &ids)),
        },
        None => selected_id(w, sender)?,
    };
    let name = name_at(w, id);
    if w.is_out(id) {
        return Err(err(w, M_ALREADY_SPAWNED, &[&name]));
    }
    let stored = w.saves.get(id).and_then(NpcData::location);
    if stored.is_none() || args.has_value("location") {
        let loc = sender
            .at
            .as_ref()
            .map(|(_, l)| l.clone())
            .ok_or_else(|| err(w, M_NO_STORED_LOCATION, &[]))?;
        npc_mut(w, id)?.set_location(&loc);
    }
    npc_mut(w, id)?.set_spawned(true);
    w.mark_dirty();
    if w.spawn(id) {
        w.select(skey(sender), id);
        return Ok(ok(w, M_SPAWNED, &[&name]));
    }
    Ok(Vec::new())
}

/// The point one block in front of `l` (the way it faces), looking back at it.
fn in_front(l: &Location) -> Location {
    let yaw = f64::from(l.yaw).to_radians();
    let pitch = f64::from(l.pitch).to_radians();
    let len = pitch.cos();
    let mut to = l.clone();
    to.x -= yaw.sin() * len;
    to.z += yaw.cos() * len;
    let back = (l.yaw + 180.0) % 360.0;
    to.yaw = back;
    to.body_yaw = back;
    to.pitch = 0.0;
    to
}

fn tp(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let player = sender.uuid.ok_or_else(|| err(w, M_MUST_BE_INGAME, &[]))?;
    let Some(loc) = w.current_location(id) else {
        return Err(err(w, M_TP_NO_LOCATION, &[]));
    };
    let Some(dim) = w.cfg.worlds.dimension(&loc.world) else {
        return Err(err(w, M_TP_NO_LOCATION, &[]));
    };
    let to = if args.has('e') { loc } else { in_front(&loc) };
    w.push_op(crate::world::Op::Console(format!(
        "execute in {dim} run tp {} {} {} {} {} {}",
        crate::ids::format_uuid(player),
        to.x,
        to.y,
        to.z,
        to.yaw,
        to.pitch
    )));
    let name = name_at(w, id);
    Ok(ok(w, M_TELEPORTED_TO_NPC, &[&name]))
}

fn tphere(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let mut to = here(w, sender)?;
    let sender_loc = to.clone();
    if args.words.first().is_some_and(|a| a.eq_ignore_ascii_case("cursor")) {
        // The block the sender looks at needs a ray cast Horizon's hooks do not give.
        return Err(err(w, M_MISSING_CURSOR_BLOCK, &[]));
    }
    if !exact(sender, w.cfg.command_level, "citizens.npc.tphere.multiworld")
        && w.saves.get(id).and_then(NpcData::location).is_some_and(|l| l.world != to.world)
    {
        return Err(err(w, M_NO_MULTIWORLD, &[]));
    }
    if args.has('c') {
        to.x = to.x.floor() + 0.5;
        to.y = to.y.floor();
        to.z = to.z.floor() + 0.5;
        to.yaw = 0.0;
        to.pitch = 0.0;
        to.body_yaw = 0.0;
    }
    if args.has('f') {
        let yaw = f64::from(to.yaw).to_radians();
        let pitch = f64::from(to.pitch).to_radians();
        to.x -= yaw.sin() * pitch.cos();
        to.z += yaw.cos() * pitch.cos();
        let back = (to.yaw + 180.0) % 360.0;
        to.yaw = back;
        to.body_yaw = back;
        to.pitch = 0.0;
    }
    if !w.is_out(id) {
        let npc = npc_mut(w, id)?;
        npc.set_location(&to);
        npc.set_spawned(true);
        w.mark_dirty();
        w.spawn(id);
    } else if !w.teleport(id, to) {
        return Err(err(w, M_WORLD_NOT_FOUND, &[]));
    }
    let name = name_at(w, id);
    Ok(ok(w, M_NPC_TELEPORTED, &[&name, &pretty_location(&sender_loc)]))
}

fn moveto(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    if !w.is_out(id) {
        npc_mut(w, id)?.set_spawned(true);
        w.spawn(id);
        if !w.is_out(id) {
            return Err(err(w, M_MOVETO_NO_SPAWN, &[]));
        }
    }
    let mut to = w.current_location(id).ok_or_else(|| err(w, M_MOVETO_NO_SPAWN, &[]))?;
    if !args.words.is_empty() {
        let joined = args.words.join(":");
        let parts: Vec<&str> = joined.split(':').collect();
        if parts.len() != 3 && parts.len() != 4 {
            return Err(err(w, M_MOVETO_FORMAT, &[]));
        }
        let num = |s: &str| s.parse::<f64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]));
        let (x, y, z) = (num(parts[0])?, num(parts[1])?, num(parts[2])?);
        if parts.len() == 4 {
            if w.cfg.worlds.dimension(parts[3]).is_none() {
                return Err(err(w, M_WORLD_NOT_FOUND, &[]));
            }
            to.world = parts[3].to_owned();
        }
        to.x = x;
        to.y = y;
        to.z = z;
    } else {
        for (flag, slot) in [("x", 0), ("y", 1), ("z", 2), ("yaw", 3), ("pitch", 4)] {
            if let Some(v) = args.value(flag) {
                let v = v.parse::<f64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
                match slot {
                    0 => to.x = v,
                    1 => to.y = v,
                    2 => to.z = v,
                    3 => {
                        to.yaw = v as f32;
                        to.body_yaw = v as f32;
                    }
                    _ => to.pitch = v as f32,
                }
            }
        }
        if let Some(world) = args.value("world") {
            if w.cfg.worlds.dimension(world).is_none() {
                return Err(err(w, M_WORLD_NOT_FOUND, &[]));
            }
            to.world = world.to_owned();
        }
    }
    if !w.teleport(id, to.clone()) {
        return Err(err(w, M_WORLD_NOT_FOUND, &[]));
    }
    let name = name_at(w, id);
    Ok(ok(w, M_MOVETO_TELEPORTED, &[&name, &pretty_location(&to)]))
}

// ---- name, skin, looks ----------------------------------------------------------------------

fn rename(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let kind = w.saves.get(id).map(NpcData::kind).unwrap_or_default();
    let mut out = Vec::new();
    let new_name = limit_name(w, args.rest(0), &kind, &mut out);
    let old = name_at(w, id);
    npc_mut(w, id)?.set_name(&new_name);
    w.mark_dirty();
    w.respawn(id);
    out.extend(ok(w, M_RENAMED, &[&old, &new_name]));
    Ok(out)
}

fn nameplate(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let npc = npc_mut(w, id)?;
    let next = if args.has('h') {
        Nameplate::Hover
    } else if npc.nameplate() == Nameplate::Shown {
        Nameplate::Hidden
    } else {
        Nameplate::Shown
    };
    npc.set_nameplate(next);
    let shown = match next {
        Nameplate::Shown => "true",
        Nameplate::Hover => "hover",
        Nameplate::Hidden => "false",
    };
    w.mark_dirty();
    w.refresh(id);
    Ok(ok(w, M_NAMEPLATE_SET, &[shown]))
}

/// The bytes of standard base64 (`=` padding allowed), or none when it is not.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.trim_end_matches('=').chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return None,
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn skin(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let (npc_name, current) = {
        let npc = w.saves.get(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
        if !npc.is_player() {
            return Err(err(w, M_DISALLOWED_MOBTYPE, &[&pretty_enum(&npc.kind())]));
        }
        (name_of(npc), npc.skin())
    };
    if args.has('c') {
        npc_mut(w, id)?.set_skin(&Skin::default());
        w.mark_dirty();
        w.respawn(id);
        return Ok(ok(w, M_SKIN_CLEARED, &[&npc_name]));
    }
    if args.has_value("url") || args.has_value("file") {
        let what = args.value("url").or_else(|| args.value("file")).unwrap_or("");
        return Err(err(w, M_SKIN_URL_ERROR, &[what]));
    }
    for (flag, what) in [('u', "-u"), ('e', "-e"), ('s', "-s"), ('l', "-l")] {
        if args.has(flag) {
            return Err(err(w, M_UNSUPPORTED, &[what]));
        }
    }
    if args.has('t') {
        // `skin -t <uuid/name> <data> <signature>`
        let [name, data, signature] = args.words.as_slice() else {
            return Err(err(w, M_SKIN_REQUIRED, &[]));
        };
        let decoded = base64_decode(data)
            .and_then(|b| String::from_utf8(b).ok())
            .filter(|j| j.contains("textures"));
        if decoded.is_none() {
            return Err(err(w, "Invalid texture data", &[]));
        }
        let npc = npc_mut(w, id)?;
        npc.set_skin(&Skin {
            name: Some(text::strip(name)),
            texture: Some(data.clone()),
            signature: Some(signature.clone()),
        });
        npc.map.put(
            &["metadata", "cached-skin-uuid-name"],
            Node::str(&name.to_ascii_lowercase()),
        );
        w.mark_dirty();
        w.respawn(id);
        return Ok(ok(w, M_SKIN_SET, &[&npc_name, name]));
    }
    let Some(skin_name) = args.words.first().cloned() else {
        // Only `/npc skin`: it tells the skin name.
        return Ok(vec![current.name.map(|n| n.to_ascii_lowercase()).unwrap_or_default()]);
    };
    let reply = ok(w, M_SKIN_SET, &[&npc_name, &skin_name]);
    npc_mut(w, id)?.set_skin(&Skin {
        name: Some(text::strip(&skin_name)),
        texture: None,
        signature: None,
    });
    w.mark_dirty();
    w.respawn(id);
    Ok(reply)
}

fn parse_range(s: &str) -> Option<(f64, f64)> {
    let mut parts = s.split(',');
    let min: f64 = parts.next()?.trim().parse().ok()?;
    let max: f64 = parts.next()?.trim().parse().ok()?;
    (min <= max).then_some((min, max))
}

fn lookclose(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    for what in [
        "filter",
        "perplayer",
        "randomswitchtargets",
        "disablewhennavigating",
        "targetnpcs",
        "headonly",
        "linkedbody",
        "randomlookdelay",
    ] {
        if args.has_value(what) {
            return Err(err(w, M_UNSUPPORTED, &[&format!("--{what}")]));
        }
    }
    if args.has('r') {
        return Err(err(w, M_UNSUPPORTED, &["-r"]));
    }
    let name = name_at(w, id);
    let random = args.value("randomlook").or_else(|| args.value("rlook"));
    let range = match args.value("range") {
        Some(r) => Some(r.parse::<f64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?),
        None => None,
    };
    // Each change says what it did, in the order Citizens checks them.
    let mut lines: Vec<(&str, Vec<String>)> = Vec::new();
    let mut bad: Option<String> = None;
    let mut toggle = true;
    let mut ranges: Vec<(bool, f64, f64)> = Vec::new();
    for (key, yaw, msg) in [("randompitchrange", false, M_LOOKCLOSE_PITCH_SET), ("randomyawrange", true, M_LOOKCLOSE_YAW_SET)] {
        if let Some(v) = args.value(key) {
            match parse_range(v) {
                Some((min, max)) => {
                    ranges.push((yaw, min, max));
                    lines.push((msg, vec![name.clone(), v.to_owned()]));
                    toggle = false;
                }
                None => {
                    bad = Some(v.to_owned());
                    break;
                }
            }
        }
    }
    let npc = npc_mut(w, id)?;
    let mut front: Vec<(&str, Vec<String>)> = Vec::new();
    if let Some(v) = random {
        let on = v.eq_ignore_ascii_case("true");
        npc.set_random_look(on);
        front.push((if on { M_LOOKCLOSE_RANDOM_SET } else { M_LOOKCLOSE_RANDOM_STOPPED }, vec![name.clone()]));
        toggle = false;
    }
    if let Some(r) = range {
        npc.set_look_range(r);
        front.push((M_LOOKCLOSE_RANGE_SET, vec![name.clone(), jnum(r)]));
        toggle = false;
    }
    for (yaw, min, max) in ranges {
        npc.set_random_range(yaw, min, max);
    }
    if bad.is_none() && toggle {
        let on = !npc.look_close();
        npc.set_look_close(on);
        front.push((if on { M_LOOKCLOSE_SET } else { M_LOOKCLOSE_STOPPED }, vec![name.clone()]));
    }
    w.mark_dirty();
    front.extend(lines);
    let mut out = Vec::new();
    for (t, a) in front {
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        out.extend(ok(w, t, &a));
    }
    if let Some(b) = bad {
        out.extend(err(w, M_LOOKCLOSE_RANGE_ERROR, &[&b]));
        return Err(out);
    }
    Ok(out)
}

fn vulnerable(w: &mut World, sender: &Sender) -> Res {
    let id = target(w, sender)?;
    let (protected, name) = {
        let npc = npc_mut(w, id)?;
        let protected = !npc.protected();
        npc.set_protected(protected);
        (protected, name_of(npc))
    };
    w.mark_dirty();
    w.refresh(id);
    // Citizens' messages read the other way round: "vulnerable" is the state it leaves.
    Ok(ok(w, if protected { M_VULNERABLE_STOPPED } else { M_VULNERABLE_SET }, &[&name]))
}

fn glowing(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let colour = args.value("color").map(str::to_ascii_lowercase);
    let name = name_at(w, id);
    if let Some(c) = &colour
        && ["bold", "italic", "underline", "underlined", "strikethrough", "magic", "obfuscated", "reset"].contains(&c.as_str())
    {
        return Err(err(w, M_GLOWING_FORMAT, &[&c.to_ascii_uppercase()]));
    }
    // A colour name Citizens cannot read is `null` there, and the command toggles.
    let colour = colour.as_deref().and_then(text::chat_color);
    let reply = if let Some(c) = colour {
        let npc = npc_mut(w, id)?;
        npc.set_team_color(c);
        if npc.map.at(&["metadata", "glowing"]).is_none() {
            npc.set_glowing(true);
        }
        let code = code_of(c).map_or(String::new(), |ch| format!("&{ch}"));
        ok(w, M_GLOWING_COLOR_SET, &[&name, &format!("{code}{}", pretty_enum(c))])
    } else {
        let on = {
            let npc = npc_mut(w, id)?;
            let on = !npc.glowing();
            npc.set_glowing(on);
            on
        };
        ok(w, if on { M_GLOWING_SET } else { M_GLOWING_UNSET }, &[&name])
    };
    w.mark_dirty();
    w.refresh(id);
    Ok(reply)
}

fn gravity(w: &mut World, sender: &Sender) -> Res {
    let id = target(w, sender)?;
    let no_gravity = {
        let npc = npc_mut(w, id)?;
        let no_gravity = !npc.no_gravity();
        npc.set_no_gravity(no_gravity);
        no_gravity
    };
    w.mark_dirty();
    w.refresh(id);
    Ok(ok(w, if no_gravity { M_GRAVITY_DISABLED } else { M_GRAVITY_ENABLED }, &[]))
}

fn set_type(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let raw = args.words.first().cloned().unwrap_or_default();
    let upper = raw.to_ascii_uppercase();
    if !valid_type(&upper) {
        return Err(err(w, M_INVALID_ENTITY_TYPE, &[&raw]));
    }
    let name = name_at(w, id);
    npc_mut(w, id)?.set_kind(&upper);
    w.mark_dirty();
    w.respawn(id);
    Ok(ok(w, M_ENTITY_TYPE_SET, &[&name, &raw]))
}

fn owner_cmd(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let name = name_at(w, id);
    let current = w.saves.get(id).and_then(NpcData::owner);
    let Some(word) = args.words.first().cloned() else {
        let who = current.map_or_else(|| "server".to_owned(), |u| w.player_name(u));
        return Ok(ok(w, M_OWNER, &[&name, &who]));
    };
    if word.eq_ignore_ascii_case("server") {
        if current.is_none() {
            return Err(err(w, M_ALREADY_OWNER, &["null", &name]));
        }
        npc_mut(w, id)?.set_server_owner();
        w.mark_dirty();
        return Ok(ok(w, M_OWNER_SET_SERVER, &[&name]));
    }
    // A name resolves through the sender, the players online and the profiles the glue looked up;
    // anything else has to be a UUID.
    let uuid = if sender.uuid.is_some() && word.eq_ignore_ascii_case(&sender.name) {
        sender.uuid
    } else {
        w.player_by_name(&word).or_else(|| crate::ids::parse_uuid(&word))
    };
    let Some(uuid) = uuid else {
        return Err(err(w, "Invalid UUID string: {0}", &[&word]));
    };
    let shown = w.player_name(uuid);
    if current == Some(uuid) {
        return Err(err(w, M_ALREADY_OWNER, &[&shown, &name]));
    }
    npc_mut(w, id)?.set_owner(uuid);
    w.mark_dirty();
    Ok(ok(w, M_OWNER_SET, &[&name, &shown]))
}

// ---- traits that show in the entity's data ----------------------------------------------------

/// Bukkit's `DyeColor` names in vanilla's colour order.
pub const DYE_COLOURS: [&str; 16] = [
    "WHITE", "ORANGE", "MAGENTA", "LIGHT_BLUE", "YELLOW", "LIME", "PINK", "GRAY", "LIGHT_GRAY", "CYAN", "PURPLE",
    "BLUE", "BROWN", "GREEN", "RED", "BLACK",
];

/// The selected NPC and its type, which must be one of `kinds` (or any but a player when empty).
fn target_of_kind(w: &World, sender: &Sender, kinds: &[&str]) -> Result<i32, Reply> {
    let id = target(w, sender)?;
    let kind = w.saves.get(id).map(NpcData::kind).unwrap_or_default().to_ascii_uppercase();
    let fits = if kinds.is_empty() { kind != "PLAYER" } else { kinds.contains(&kind.as_str()) };
    if !fits {
        return Err(err(w, M_REQ_MOBTYPE, &[&pretty_enum(&kind)]));
    }
    Ok(id)
}

fn age(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = selected_id(w, sender)?;
    if !owns(w, sender, id) {
        return Err(err(w, M_MUST_OWN, &[]));
    }
    let name = name_at(w, id);
    // Guess: Citizens asks for an `Ageable`/`Zombie`; any mob but a player passes here.
    let kind = w.saves.get(id).map(NpcData::kind).unwrap_or_default();
    if kind.eq_ignore_ascii_case("PLAYER") || !w.is_out(id) {
        return Err(err(w, M_AGE_CANNOT, &[&name]));
    }
    let mut out = Vec::new();
    let lock = args.has('l');
    let (age, locked) = {
        let npc = npc_mut(w, id)?;
        let age = npc.map.at(&["traits", "age", "age"]).and_then(Node::as_i64).unwrap_or(0);
        let locked = npc.map.at(&["traits", "age", "locked"]).and_then(Node::as_bool).unwrap_or(true);
        (age, locked)
    };
    if lock {
        npc_mut(w, id)?.map.put(&["traits", "age", "locked"], Node::bool(!locked));
        out.extend(ok(w, if !locked { M_AGE_LOCKED } else { M_AGE_UNLOCKED }, &[]));
    }
    let Some(arg) = args.words.first() else {
        if !lock {
            out.extend(ok(w, M_AGE_DESCRIBE, &[&name, &age.to_string(), &(if lock { !locked } else { locked }).to_string()]));
        }
        return Ok(out);
    };
    let new_age = match arg.parse::<i64>() {
        Ok(n) if n > 0 => return Err(err(w, M_AGE_INVALID, &[])),
        Ok(n) => {
            out.extend(ok(w, M_AGE_NORMAL, &[&name, &n.to_string()]));
            n
        }
        Err(_) if arg.eq_ignore_ascii_case("baby") => {
            out.extend(ok(w, M_AGE_BABY, &[&name]));
            -24000
        }
        Err(_) if arg.eq_ignore_ascii_case("adult") => {
            out.extend(ok(w, M_AGE_ADULT, &[&name]));
            0
        }
        Err(_) => return Err(err(w, M_AGE_INVALID, &[])),
    };
    {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "age", "age"], Node::int(new_age));
        if npc.map.at(&["traits", "age", "locked"]).is_none() {
            npc.map.put(&["traits", "age", "locked"], Node::bool(true));
        }
        npc.add_trait_name("age");
    }
    w.mark_dirty();
    w.respawn(id);
    Ok(out)
}

fn powered(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target_of_kind(w, sender, &["CREEPER"])?;
    let name = name_at(w, id);
    let cur = w.saves.get(id).and_then(|n| n.map.at(&["traits", "powered"]).and_then(Node::as_bool)).unwrap_or(false);
    let value = match args.value("set") {
        Some(v) => v.eq_ignore_ascii_case("true"),
        None => !cur,
    };
    {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "powered"], Node::bool(value));
        npc.add_trait_name("powered");
    }
    w.mark_dirty();
    w.respawn(id);
    // Citizens sends these two without their argument.
    Ok(ok(w, if value { M_POWERED_SET } else { M_POWERED_STOPPED }, &[]).into_iter().map(|l| l.replace("{0}", &name)).collect())
}

fn sheep(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target_of_kind(w, sender, &["SHEEP"])?;
    let mut out = Vec::new();
    let mut had_arg = false;
    if let Some(v) = args.value("sheared") {
        let sheared = v.eq_ignore_ascii_case("true");
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "sheeptrait", "sheared"], Node::bool(sheared));
        npc.add_trait_name("sheeptrait");
        had_arg = true;
    }
    if let Some(v) = args.value("color") {
        let upper = v.to_ascii_uppercase().replace(' ', "_");
        if DYE_COLOURS.contains(&upper.as_str()) {
            let npc = npc_mut(w, id)?;
            npc.map.put(&["traits", "sheeptrait", "color"], Node::str(&upper));
            npc.add_trait_name("sheeptrait");
            out.extend(ok(w, M_SHEEP_COLOR_SET, &[&pretty_enum(&upper)]));
        } else {
            let valid: Vec<String> = DYE_COLOURS.iter().map(|c| pretty_enum(c)).collect();
            out.extend(err(w, M_SHEEP_INVALID_COLOR, &[&valid.join(", ")]));
        }
        had_arg = true;
    }
    if !had_arg {
        return Err(Vec::new());
    }
    w.mark_dirty();
    w.respawn(id);
    Ok(out)
}

fn slimesize(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target_of_kind(w, sender, &["SLIME", "MAGMA_CUBE"])?;
    let name = name_at(w, id);
    let Some(raw) = args.words.first() else {
        let size = w
            .saves
            .get(id)
            .and_then(|n| n.map.at(&["traits", "slimesize", "size"]).and_then(Node::as_i64))
            .unwrap_or(3);
        return Ok(ok(w, M_SIZE_DESCRIBE, &[&name, &size.to_string()]));
    };
    let size = parse_i64(w, raw)?.max(-2);
    {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "slimesize", "size"], Node::int(size));
        npc.add_trait_name("slimesize");
    }
    w.mark_dirty();
    w.respawn(id);
    Ok(ok(w, M_SIZE_SET, &[&name, &size.to_string()]))
}

// ---- movement and looking ------------------------------------------------------------------

/// Java's `Float.toString` for a value a command prints back.
fn jfloat(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e7 { format!("{v:.1}") } else { v.to_string() }
}

/// An online player by name (any case) or UUID.
fn online_player<'a>(w: &'a World, raw: &str) -> Option<&'a crate::look::PlayerView> {
    let uuid = if raw.len() == 36 { crate::ids::parse_uuid(raw) } else { None };
    w.online
        .iter()
        .find(|p| Some(p.uuid) == uuid || p.name.eq_ignore_ascii_case(raw))
}

fn follow(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let name = name_at(w, id);
    if let Some(m) = args.value("margin") {
        let m: f64 = m.parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
        npc_mut(w, id)?.map.put(&["traits", "followtrait", "margin"], Node::float(m));
        npc_mut(w, id)?.add_trait_name("followtrait");
        w.mark_dirty();
        w.cancel_follow_walk(id);
        return Ok(ok(w, M_FOLLOW_MARGIN, &[&name, &jnum(m)]));
    }
    if args.has('p') {
        return Err(err(w, M_UNSUPPORTED, &["-p"]));
    }
    let level = w.cfg.command_level;
    let others = allowed(sender, level, "citizens.npc.follow.others");
    let who = args.words.first().cloned().unwrap_or_else(|| sender.name.clone());
    let enabled = w.saves.get(id).is_some_and(|n| n.follow_target().is_some());
    let enable_flag = args.value("enable").map(|v| v.eq_ignore_ascii_case("true"));
    let following = enable_flag.unwrap_or(!enabled);
    let (uuid, shown) = match online_player(w, &who).map(|p| (p.uuid, p.name.clone())) {
        Some((u, n)) => {
            if Some(u) != sender.uuid && !others {
                return Err(err(w, M_NO_PERM, &[]));
            }
            (u, n)
        }
        None if others => match find(w, &who, None, sender) {
            Found::One(other) => {
                if !owns(w, sender, other) {
                    return Err(err(w, M_MUST_OWN, &[]));
                }
                let u = w.saves.get(other).and_then(NpcData::uuid).ok_or_else(|| err(w, M_MUST_SELECT, &[]))?;
                (u, name_at(w, other))
            }
            Found::Missing => return Err(err(w, M_MUST_SELECT, &[])),
            _ => return Ok(Vec::new()),
        },
        None => return Err(err(w, M_NPC_NOT_FOUND, &[])),
    };
    npc_mut(w, id)?.set_follow(if following { Some(uuid) } else { None });
    w.mark_dirty();
    w.cancel_follow_walk(id);
    Ok(ok(w, if following { M_FOLLOW_SET } else { M_FOLLOW_UNSET }, &[&name, &shown]))
}

fn speed(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let v: f64 = args.words[0].parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
    let v = (v as f32).abs();
    npc_mut(w, id)?.map.put(&["navigator", "speedmodifier"], Node::float(f64::from(v)));
    w.mark_dirty();
    Ok(ok(w, M_SPEED_SET, &[&jfloat(v)]))
}

fn pathto(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    if args.has_value("margin") {
        return Err(err(w, M_UNSUPPORTED, &["--margin"]));
    }
    let mut to = w.current_location(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
    if args.words.len() == 1 {
        match args.words[0].to_ascii_lowercase().as_str() {
            "me" | "here" => to = here(w, sender)?,
            "cursor" => return Err(err(w, M_UNSUPPORTED, &["pathto cursor"])),
            _ => return Err(usage_of(w, "pathto")),
        }
    } else {
        let num = |s: &String| s.parse::<f64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]));
        to.x = num(&args.words[0])?;
        to.y = num(args.words.get(1).ok_or_else(|| usage_of(w, "pathto"))?)?;
        to.z = num(args.words.get(2).ok_or_else(|| usage_of(w, "pathto"))?)?;
    }
    let speed = 0.2 * w.saves.get(id).map_or(1.0, NpcData::speed_modifier);
    // Straight line: Citizens' `-s` mode. Its pathfinder needs a mob with AI.
    w.navigate_to(id, [to.x, to.y, to.z], speed);
    Ok(Vec::new())
}

fn parse_xyz(s: &str) -> Option<[f64; 3]> {
    let p: Vec<&str> = s.split([',', ':']).collect();
    if p.len() < 3 {
        return None;
    }
    Some([p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?])
}

fn rotate(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    if args.has('s') {
        return Err(err(w, M_UNSUPPORTED, &["rotate -s"]));
    }
    let num = |w: &World, k: &str| -> Result<Option<f32>, Reply> {
        match args.value(k) {
            Some(v) => v.parse::<f32>().map(Some).map_err(|_| err(w, M_INVALID_NUMBER, &[])),
            None => Ok(None),
        }
    };
    let (yaw, head, pitch) = (num(w, "body")?, num(w, "head")?, num(w, "pitch")?);
    let at = w.current_location(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
    let mut towards: Option<[f64; 3]> = match args.value("towards") {
        Some(t) => Some(parse_xyz(t).ok_or_else(|| err(w, M_INVALID_LOCATION, &[]))?),
        None => None,
    };
    if let Some(e) = args.value("toentity") {
        towards = if e == "me" {
            Some(sender.at.as_ref().map(|(_, l)| [l.x, l.y, l.z]).ok_or_else(|| err(w, M_MUST_BE_INGAME, &[]))?)
        } else {
            Some(online_player(w, e).map(|p| p.pos).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?)
        };
    }
    if let Some(t) = towards {
        let (y, p) = crate::look::look_at(crate::look::eye([at.x, at.y, at.z]), t);
        w.face(id, y, p);
        return Ok(Vec::new());
    }
    // Head and body turn together here (the console `rotate`); Citizens can turn the head alone.
    let y = yaw.or(head).unwrap_or(at.yaw);
    if yaw.is_some() || head.is_some() || pitch.is_some() {
        w.face(id, y, pitch.unwrap_or(at.pitch));
    }
    Ok(Vec::new())
}

fn tpto(w: &mut World, sender: &Sender, args: &Args) -> Res {
    #[derive(Clone)]
    enum Who {
        Npc(i32),
        Player(u128),
    }
    let sel = w.selected(skey(sender)).map(Who::Npc);
    let by_id = |w: &World, s: &str| s.parse::<i32>().ok().filter(|i| w.saves.get(*i).is_some()).map(Who::Npc);
    let by_name = |w: &World, s: &str| online_player(w, s).map(|p| Who::Player(p.uuid));
    let (mut from, mut to) = (sel, None);
    let first_is_number = args.words[0].parse::<i32>().is_ok();
    let two = args.words.len() == 1;
    if first_is_number {
        if let Some(n) = by_id(w, &args.words[0]) {
            if two { to = Some(n) } else { from = Some(n) }
        }
    } else if two {
        to = by_name(w, &args.words[0]);
    } else {
        from = by_name(w, &args.words[0]);
    }
    if let Some(second) = args.words.get(1) {
        if second.parse::<i32>().is_ok() {
            if let Some(n) = by_id(w, second) {
                to = Some(n);
            }
        } else if first_is_number {
            // `first_was_player` is false: the second may be a player.
            to = by_name(w, second);
        }
    }
    let from = from.ok_or_else(|| err(w, M_TPTO_FROM, &[]))?;
    let to = to.ok_or_else(|| err(w, M_TPTO_TO, &[]))?;
    let (dim, pos) = match &to {
        Who::Npc(i) => {
            let l = w.current_location(*i).ok_or_else(|| err(w, M_TPTO_TO, &[]))?;
            (w.dimension_of(*i).ok_or_else(|| err(w, M_TPTO_TO, &[]))?, l)
        }
        Who::Player(u) => {
            let (d, p) = w.locate(*u).ok_or_else(|| err(w, M_TPTO_TO, &[]))?;
            let world = w.cfg.worlds.world_name(&d);
            (d, Location { world, x: p[0], y: p[1], z: p[2], yaw: 0.0, pitch: 0.0, body_yaw: 0.0 })
        }
    };
    match from {
        Who::Npc(i) => {
            if !w.teleport(i, pos) {
                return Err(err(w, M_WORLD_NOT_FOUND, &[]));
            }
        }
        Who::Player(u) => w.push_op(crate::world::Op::Console(format!(
            "execute in {dim} run tp {} {} {} {}",
            crate::ids::format_uuid(u),
            pos.x,
            pos.y,
            pos.z
        ))),
    }
    Ok(ok(w, M_TPTO_OK, &[]))
}

/// `name;world;x;y;z`, as Citizens' `Anchor.stringValue` writes it.
fn anchor_parts(s: &str) -> Option<(String, Location)> {
    let p: Vec<&str> = s.split(';').collect();
    if p.len() < 5 {
        return None;
    }
    Some((
        p[0].to_owned(),
        Location { world: p[1].to_owned(), x: p[2].parse().ok()?, y: p[3].parse().ok()?, z: p[4].parse().ok()?, yaw: 0.0, pitch: 0.0, body_yaw: 0.0 },
    ))
}

fn anchor(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let path = ["traits", "anchors", "list"];
    let mut list = w.saves.get(id).map(|n| n.string_list(&path)).unwrap_or_default();
    let find_anchor = |list: &[String], name: &str| {
        list.iter().position(|a| a.split(';').next().is_some_and(|n| n.eq_ignore_ascii_case(name)))
    };
    let mut out = Vec::new();
    if let Some(name) = args.value("save") {
        if name.is_empty() {
            return Err(err(w, M_ANCHOR_INVALID, &[]));
        }
        let loc = here(w, sender)?;
        if args.has('c') {
            return Err(err(w, M_UNSUPPORTED, &["anchor -c"]));
        }
        if find_anchor(&list, name).is_some() {
            return Err(err(w, M_ANCHOR_EXISTS, &[name]));
        }
        list.push(format!("{name};{};{};{};{}", loc.world, jnum(loc.x), jnum(loc.y), jnum(loc.z)));
        npc_mut(w, id)?.set_string_list(&path, &list);
        npc_mut(w, id)?.add_trait_name("anchors");
        w.mark_dirty();
        out.extend(ok(w, M_ANCHOR_ADDED, &[]));
    } else if let Some(name) = args.value("assume") {
        if name.is_empty() {
            return Err(err(w, M_ANCHOR_INVALID, &[]));
        }
        let Some((_, loc)) = find_anchor(&list, name).and_then(|i| anchor_parts(&list[i])) else {
            return Err(err(w, M_ANCHOR_MISSING, &[name]));
        };
        if !w.teleport(id, loc) {
            return Err(err(w, M_WORLD_NOT_FOUND, &[]));
        }
    } else if let Some(name) = args.value("remove") {
        if name.is_empty() {
            return Err(err(w, M_ANCHOR_INVALID, &[]));
        }
        let Some(i) = find_anchor(&list, name) else {
            return Err(err(w, M_ANCHOR_MISSING, &[name]));
        };
        list.remove(i);
        npc_mut(w, id)?.set_string_list(&path, &list);
        w.mark_dirty();
        out.extend(ok(w, M_ANCHOR_REMOVED, &[]));
    } else if !args.has('a') {
        let page = match args.words.first() {
            Some(p) => p.parse::<i64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?,
            None => 1,
        };
        let mut lines = vec!["Key: [[ID]]  <blue>Name  <red>World  <gray>Location (X,Y,Z)".to_owned()];
        for (i, a) in list.iter().enumerate() {
            if let Some((n, l)) = anchor_parts(a) {
                lines.push(format!("{i}<blue>  {n}<yellow>  {}<gray>  {}, {}, {}", l.world, l.x.floor() as i64, l.y.floor() as i64, l.z.floor() as i64));
            }
        }
        const PER_PAGE: usize = 9;
        let pages = lines.len().div_ceil(PER_PAGE) as i64;
        if page < 1 || page > pages {
            return Err(err(w, M_PAGE_MISSING, &[&page.to_string()]));
        }
        out.extend(say(w, &header(&format!("Anchors {page}/{pages}"))));
        for l in lines.iter().skip((page as usize - 1) * PER_PAGE).take(PER_PAGE) {
            out.extend(say(w, l));
        }
        return Ok(out);
    }
    if args.has('a') {
        let loc = here(w, sender)?;
        w.teleport(id, loc);
    }
    Ok(out)
}

fn pose(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let path = ["traits", "poses", "list"];
    let mut list = w.saves.get(id).map(|n| n.string_list(&path)).unwrap_or_default();
    let has = |list: &[String], name: &str| list.iter().any(|p| p.split(';').next().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    let num = |k: &str| -> Result<Option<f32>, Reply> {
        match args.value(k) {
            Some(v) => v.parse::<f32>().map(Some).map_err(|_| err(w, M_INVALID_NUMBER, &[])),
            None => Ok(None),
        }
    };
    let (yaw_flag, pitch_flag) = (num("yaw")?, num("pitch")?);
    let mut out = Vec::new();
    let mut add = |w: &mut World, name: &str, yaw: f32, pitch: f32, default: bool, list: &mut Vec<String>| -> Result<(), Reply> {
        if name.is_empty() {
            return Err(err(w, M_POSE_INVALID, &[]));
        }
        let lower = name.to_ascii_lowercase();
        if has(list, &lower) {
            return Err(err(w, M_POSE_EXISTS, &[name]));
        }
        list.push(format!("{lower};{};{}", jfloat(pitch), jfloat(yaw)));
        let npc = npc_mut(w, id)?;
        npc.set_string_list(&path, list);
        npc.add_trait_name("poses");
        if default {
            npc.map.put(&["traits", "poses", "defaultPose"], Node::str(&lower));
        }
        w.mark_dirty();
        Ok(())
    };
    if let Some(name) = args.value("save") {
        let at = w.current_location(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
        add(w, name, yaw_flag.unwrap_or(at.yaw), pitch_flag.unwrap_or(at.pitch), args.has('d'), &mut list)?;
        out.extend(ok(w, M_POSE_ADDED, &[]));
    } else if let Some(name) = args.value("mirror") {
        let at = here(w, sender)?;
        add(w, name, at.yaw, at.pitch, args.has('d'), &mut list)?;
        out.extend(ok(w, M_POSE_ADDED, &[]));
    } else if let Some(name) = args.value("default") {
        if !has(&list, name) {
            return Err(err(w, M_POSE_MISSING, &[name]));
        }
        npc_mut(w, id)?.map.put(&["traits", "poses", "defaultPose"], Node::str(name));
        w.mark_dirty();
        out.extend(ok(w, M_POSE_DEFAULT, &[name]));
    } else if let Some(name) = args.value("assume") {
        if name.is_empty() {
            return Err(err(w, M_POSE_INVALID, &[]));
        }
        let found = list.iter().find_map(|p| {
            let mut it = p.split(';');
            let (n, pitch, yaw) = (it.next()?, it.next()?, it.next()?);
            n.eq_ignore_ascii_case(name).then(|| Some((yaw.parse::<f32>().ok()?, pitch.parse::<f32>().ok()?)))?
        });
        let Some((yaw, pitch)) = found else {
            return Err(err(w, M_POSE_MISSING, &[name]));
        };
        assume_pose(w, id, yaw, pitch);
    } else if let Some(name) = args.value("remove") {
        if name.is_empty() {
            return Err(err(w, M_POSE_INVALID, &[]));
        }
        let Some(i) = list.iter().position(|p| p.split(';').next().is_some_and(|n| n.eq_ignore_ascii_case(name))) else {
            return Err(err(w, M_POSE_MISSING, &[name]));
        };
        list.remove(i);
        npc_mut(w, id)?.set_string_list(&path, &list);
        w.mark_dirty();
        out.extend(ok(w, M_POSE_REMOVED, &[]));
    } else if !args.has('a') {
        let page = match args.words.first() {
            Some(p) => p.parse::<i64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?,
            None => 1,
        };
        let mut lines = vec!["<green>ID  <yellow>Name  <red>Pitch/Yaw".to_owned()];
        for (i, p) in list.iter().enumerate() {
            let mut it = p.split(';');
            if let (Some(n), Some(pitch), Some(yaw)) = (it.next(), it.next(), it.next()) {
                lines.push(format!("<green>{i}<yellow>  {n}<red>  {pitch} {yaw}"));
            }
        }
        const PER_PAGE: usize = 9;
        let pages = lines.len().div_ceil(PER_PAGE) as i64;
        if page < 1 || page > pages {
            return Err(err(w, M_PAGE_MISSING, &[&page.to_string()]));
        }
        out.extend(say(w, &header(&format!("Pose {page}/{pages}"))));
        for l in lines.iter().skip((page as usize - 1) * PER_PAGE).take(PER_PAGE) {
            out.extend(say(w, l));
        }
        return Ok(out);
    }
    if args.has('a') {
        let at = here(w, sender)?;
        assume_pose(w, id, at.yaw, at.pitch);
    }
    Ok(out)
}

fn assume_pose(w: &mut World, id: i32, yaw: f32, pitch: f32) {
    if !w.is_out(id) {
        if let Some(n) = w.saves.get_mut(id) {
            n.set_spawned(true);
        }
        w.spawn(id);
    }
    w.face(id, yaw, pitch);
}

fn hurt(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let kind = w.saves.get(id).map(NpcData::kind).unwrap_or_default();
    if !w.is_out(id) {
        return Err(err(w, M_HURT_NOT_DAMAGEABLE, &[&pretty_enum(&kind)]));
    }
    if w.saves.get(id).is_some_and(NpcData::protected) {
        return Err(err(w, M_HURT_PROTECTED, &[]));
    }
    let amount = parse_i64(w, &args.words[0])?;
    let uuid = w.saves.get(id).and_then(NpcData::uuid).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
    let dim = w.dimension_of(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
    w.push_op(crate::world::Op::Console(format!(
        "execute in {dim} run damage {} {amount}",
        crate::ids::format_uuid(uuid)
    )));
    Ok(Vec::new())
}

fn sound(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let name = name_at(w, id);
    let get = |w: &World, k: &str| w.saves.get(id).and_then(|n| n.map.at(&["metadata", &format!("{k}-sound")]).and_then(Node::text));
    let (mut ambient, mut hurt, mut death) = (get(w, "ambient"), get(w, "hurt"), get(w, "death"));
    let silent = |w: &World| w.saves.get(id).and_then(|n| n.map.at(&["metadata", "silent"]).and_then(Node::as_bool)).unwrap_or(false);
    if args.values.is_empty() && args.switches.is_empty() {
        let shown = |s: &Option<String>| s.clone().unwrap_or_else(|| "null".to_owned());
        return Ok(ok(w, M_SOUND_INFO, &[&name, &shown(&ambient), &shown(&hurt), &shown(&death), "any sound id"]));
    }
    let mut set_silent: Option<bool> = None;
    if args.has('n') {
        (ambient, hurt, death) = (Some(String::new()), Some(String::new()), Some(String::new()));
        set_silent = Some(true);
    }
    if args.has('s') {
        let now = !set_silent.unwrap_or_else(|| silent(w));
        set_silent = Some(now);
        if now {
            (ambient, hurt, death) = (Some(String::new()), Some(String::new()), Some(String::new()));
        } else {
            (ambient, hurt, death) = (None, None, None);
        }
        // `-s` toggles: the value just computed is the new state.
        set_silent = Some(now);
    }
    if args.has('d') {
        (ambient, hurt, death) = (None, None, None);
        set_silent = Some(false);
    } else {
        for (key, slot) in [("death", &mut death), ("ambient", &mut ambient), ("hurt", &mut hurt)] {
            if let Some(v) = args.value(key) {
                *slot = if v == "d" { None } else { Some(v.to_owned()) };
            }
        }
    }
    let on = |s: &Option<String>| s.as_ref().is_some_and(|v| !v.is_empty() && v != "none");
    if on(&ambient) || on(&death) || on(&hurt) {
        set_silent = Some(false);
    }
    {
        let npc = npc_mut(w, id)?;
        for (k, v) in [("ambient", &ambient), ("hurt", &hurt), ("death", &death)] {
            match v {
                Some(v) => npc.map.put(&["metadata", &format!("{k}-sound")], Node::str(v)),
                None => {
                    npc.map.drop_path(&["metadata", &format!("{k}-sound")]);
                }
            }
        }
        if let Some(s) = set_silent {
            npc.map.put(&["metadata", "silent"], Node::bool(s));
        }
    }
    w.mark_dirty();
    let show = |s: &Option<String>| match s {
        Some(v) if v.is_empty() => "none".to_owned(),
        Some(v) => v.clone(),
        None => "null".to_owned(),
    };
    Ok(ok(w, M_SOUND_SET, &[&name, &show(&ambient), &show(&hurt), &show(&death)]))
}

fn playsound(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let sound = args.words[0].clone();
    let category = match args.words.get(1).map(|c| c.to_ascii_lowercase()).as_deref() {
        None => "master",
        Some("master") => "master",
        Some("music") => "music",
        Some("records") => "record",
        Some("weather") => "weather",
        Some("blocks") => "block",
        Some("hostile") => "hostile",
        Some("neutral") => "neutral",
        Some("players") => "player",
        Some("ambient") => "ambient",
        Some("voice") => "voice",
        Some(_) => return Err(usage_of(w, "playsound")),
    };
    let f = |i: usize| -> Result<f32, Reply> {
        match args.words.get(i) {
            Some(v) => v.parse::<f32>().map_err(|_| err(w, M_INVALID_NUMBER, &[])),
            None => Ok(1.0),
        }
    };
    let (volume, pitch) = (f(2)?, f(3)?);
    let at = match args.value("at") {
        Some(a) => parse_at(w, a, sender).ok_or_else(|| err(w, M_INVALID_LOCATION, &[]))?,
        None => w.saves.get(id).and_then(NpcData::location).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?,
    };
    let Some(dim) = w.cfg.worlds.dimension(&at.world) else {
        return Err(err(w, M_WORLD_NOT_FOUND, &[]));
    };
    let targets = match args.value("to") {
        Some(t) => match online_player(w, t).map(|p| p.uuid) {
            Some(u) => crate::ids::format_uuid(u),
            None => return Ok(Vec::new()),
        },
        None => "@a".to_owned(),
    };
    // Vanilla's range is the volume; a player farther away than that hears nothing.
    w.push_op(crate::world::Op::Console(format!(
        "execute in {dim} run playsound {sound} {category} {targets} {} {} {} {volume} {pitch}",
        at.x, at.y, at.z
    )));
    Ok(Vec::new())
}

fn wander(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let base = ["traits", "waypoints"];
    let provider = w
        .saves
        .get(id)
        .and_then(|n| n.map.at(&["traits", "waypoints", "provider"]).and_then(Node::text))
        .unwrap_or_else(|| "linear".to_owned());
    let set_provider = |w: &mut World, name: &str| -> Result<(), Reply> {
        let npc = npc_mut(w, id)?;
        npc.map.put(&["traits", "waypoints", "provider"], Node::str(name));
        npc.add_trait_name("waypoints");
        w.mark_dirty();
        w.cancel_follow_walk(id);
        Ok(())
    };
    let Some(action) = args.words.first().map(|a| a.to_ascii_lowercase()) else {
        let next = if provider == "wander" { "linear" } else { "wander" };
        set_provider(w, next)?;
        return Ok(ok(w, M_PROVIDER_SET, &[next]));
    };
    if provider != "wander" {
        set_provider(w, "wander")?;
    }
    let n = args.words.len() + 1;
    let int = |w: &World, i: usize| parse_i64(w, &args.words[i]);
    let _ = base;
    match action.as_str() {
        "xyrange" => {
            if n != 4 {
                return Err(usage_of(w, "wander"));
            }
            let (x, y) = (int(w, 1)?, int(w, 2)?);
            let npc = npc_mut(w, id)?;
            npc.map.put(&["traits", "waypoints", "wander", "xrange"], Node::int(x));
            npc.map.put(&["traits", "waypoints", "wander", "yrange"], Node::int(y));
            w.mark_dirty();
            Ok(ok(w, M_WANDER_RANGE, &[&x.to_string(), &y.to_string()]))
        }
        "delay" => {
            if n != 3 {
                return Err(usage_of(w, "wander"));
            }
            let d = int(w, 1)?;
            npc_mut(w, id)?.map.put(&["traits", "waypoints", "wander", "delay"], Node::int(d));
            w.mark_dirty();
            Ok(ok(w, M_WANDER_DELAY, &[&d.to_string()]))
        }
        "pathfind" => {
            if n != 3 {
                return Err(usage_of(w, "wander"));
            }
            let b = args.words[1].eq_ignore_ascii_case("true");
            npc_mut(w, id)?.map.put(&["traits", "waypoints", "wander", "pathfind"], Node::bool(b));
            w.mark_dirty();
            Ok(ok(w, M_WANDER_PATHFIND, &[&b.to_string()]))
        }
        "add" | "worldguardregion" => Err(err(w, M_UNSUPPORTED, &[&format!("wander {action}")])),
        _ => Ok(Vec::new()),
    }
}

// ---- hologram -------------------------------------------------------------------------------

/// A hologram line number: `bottom`, `top`, or a number (below 0 is 0).
fn line_index(arg: &str, len: usize) -> Option<i64> {
    match arg {
        "bottom" => Some(0),
        "top" => Some(len as i64 - 1),
        n => n.parse::<i64>().ok().map(|v| v.max(0)),
    }
}

fn hologram(w: &mut World, sender: &Sender, tail: &str) -> Res {
    let id = target(w, sender)?;
    let words: Vec<&str> = tail.split_whitespace().collect();
    let name = name_at(w, id);
    let len = w.saves.get(id).map_or(0, |n| n.hologram_lines().len());
    let Some(action) = words.first().map(|a| a.to_ascii_lowercase()) else {
        let npc = w.saves.get(id).ok_or_else(|| err(w, M_NPC_NOT_FOUND, &[]))?;
        let mut s = fill(M_HOLO_HEADER, &[&name]);
        for (i, l) in npc.hologram_lines().iter().enumerate() {
            s.push_str(&format!("<br>    [[{i}]] - {}", l.text));
        }
        return Ok(render(w, false, &s));
    };
    // The text after `n` words.
    let after = |n: usize| -> String { words.get(n..).map(|x| x.join(" ")).unwrap_or_default() };
    // A line number that exists (`max` is how many lines may be named).
    let line = |w: &World, at: usize, max: usize| -> Result<usize, Reply> {
        let Some(a) = words.get(at) else {
            return Err(err(w, M_HOLO_INVALID_LINE, &[]));
        };
        let idx = line_index(a, len).ok_or_else(|| err(w, M_INVALID_NUMBER, &[]))?;
        if idx < 0 || idx as usize >= max {
            return Err(err(w, M_HOLO_INVALID_LINE, &[]));
        }
        Ok(idx as usize)
    };
    let reply = match action.as_str() {
        "set" => {
            let idx = line(w, 1, len)?;
            if words.len() == 2 {
                return Err(err(w, M_HOLO_TEXT_MISSING, &[]));
            }
            let text = after(2);
            npc_mut(w, id)?.set_hologram_line(idx, &text);
            ok(w, M_HOLO_LINE_SET, &[&idx.to_string(), &text])
        }
        "add" => {
            if words.len() == 1 {
                return Err(err(w, M_HOLO_TEXT_MISSING, &[]));
            }
            if words.iter().any(|t| t.eq_ignore_ascii_case("--duration")) {
                return Err(err(w, M_UNSUPPORTED, &["--duration"]));
            }
            let text = after(1);
            npc_mut(w, id)?.add_hologram_line(&text);
            ok(w, M_HOLO_LINE_ADD, &[&text])
        }
        "insert" => {
            if words.len() == 1 {
                return Err(err(w, M_HOLO_INVALID_LINE, &[]));
            }
            if words.len() == 2 {
                return Err(err(w, M_HOLO_TEXT_MISSING, &[]));
            }
            let idx = line(w, 1, len + 1)?;
            let text = after(2);
            npc_mut(w, id)?.insert_hologram_line(idx, &text);
            ok(w, M_HOLO_LINE_ADD, &[&text])
        }
        "remove" => {
            let idx = line(w, 1, len)?;
            npc_mut(w, id)?.remove_hologram_line(idx);
            ok(w, M_HOLO_LINE_REMOVED, &[&idx.to_string()])
        }
        "clear" => {
            npc_mut(w, id)?.clear_hologram();
            ok(w, M_HOLO_CLEARED, &[])
        }
        "lineheight" => {
            let Some(h) = words.get(1) else {
                return Err(usage_of(w, "hologram"));
            };
            let h: f64 = h.parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
            npc_mut(w, id)?
                .map
                .put(&["traits", "hologramtrait", "lineHeight"], Node::float(h));
            ok(w, M_HOLO_HEIGHT_SET, &[&jnum(h)])
        }
        "margintop" | "marginbottom" => {
            let idx = line(w, 1, len)?;
            let Some(m) = words.get(2) else {
                return Err(err(w, M_HOLO_MARGIN_MISSING, &[]));
            };
            let margin: f64 = m.parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
            let top = action == "margintop";
            npc_mut(w, id)?.set_hologram_margin(idx, top, margin);
            ok(
                w,
                M_HOLO_MARGIN_SET,
                &[&idx.to_string(), if top { "top" } else { "bottom" }, &jnum(margin)],
            )
        }
        "viewrange" | "verticalviewrange" | "updaterate" | "bgcolor" | "edit_npc" => {
            return Err(err(w, M_UNSUPPORTED, &[&format!("hologram {action}")]));
        }
        _ => return Err(usage_of(w, "hologram")),
    };
    w.mark_dirty();
    w.refresh_holograms(id);
    Ok(reply)
}

// ---- command trait --------------------------------------------------------------------------

fn parse_i64(w: &World, s: &str) -> Result<i64, Reply> {
    s.parse::<i64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))
}

fn command(w: &mut World, sender: &Sender, tail: &str) -> Res {
    let id = target(w, sender)?;
    let level = w.cfg.command_level;
    let name = name_at(w, id);
    let (op, rest) = tail.trim().split_once(' ').unwrap_or((tail.trim(), ""));
    match op.to_ascii_lowercase().as_str() {
        "" => {
            let cmds = w.saves.get(id).map(NpcData::commands).unwrap_or_default();
            let describe = |c: &crate::npc::NpcCommand| {
                let cd = if c.cooldown != 0 { c.cooldown } else { i64::from(w.cfg.commands.global_cooldown_ticks / 20) };
                let cost = if c.cost > 0.0 { jnum(c.cost) } else { "default".to_owned() };
                let exp = if c.experience_cost > 0 { c.experience_cost.to_string() } else { "default".to_owned() };
                let mut s = fill(
                    M_CMD_DESCRIBE,
                    &[
                        &c.command,
                        &format!("<yellow>{cd}</yellow>"),
                        &format!("<yellow>{cost}</yellow>"),
                        &format!("<yellow>{exp}</yellow>"),
                        &c.key,
                    ],
                );
                if c.global_cooldown > 0 {
                    s.push_str(&format!("[global <yellow>{}</yellow>s]", c.global_cooldown));
                }
                if c.delay > 0 {
                    s.push_str(&format!("[delay <yellow>{}</yellow>t]", c.delay));
                }
                if c.n > 0 {
                    s.push_str(&format!("[<yellow>{}</yellow> uses]", c.n));
                }
                if c.gn > 0 {
                    s.push_str(&format!("[<yellow>{}</yellow> global uses]", c.gn));
                }
                if c.op {
                    s.push_str(" -o");
                }
                if c.player {
                    s.push_str(" -p");
                }
                s
            };
            let left: Vec<_> = cmds.iter().filter(|c| matches!(c.hand, Hand::Left | Hand::ShiftLeft | Hand::Both)).collect();
            let right: Vec<_> = cmds.iter().filter(|c| matches!(c.hand, Hand::Right | Hand::ShiftRight | Hand::Both)).collect();
            let mut parts: Vec<String> = Vec::new();
            if !left.is_empty() {
                parts.push(fill(M_CMD_LEFT, &[]));
                parts.extend(left.into_iter().map(describe));
            }
            if !right.is_empty() {
                parts.push(fill(M_CMD_RIGHT, &[]));
                parts.extend(right.into_iter().map(describe));
            }
            if parts.is_empty() {
                parts.push(fill(M_CMD_NONE, &[]));
            } else {
                parts.insert(0, "Linear".to_owned());
            }
            Ok(render(w, false, parts.join(" ").trim()))
        }
        "add" => {
            let a = Args::parse(rest);
            let command = a.rest(0);
            if command.is_empty() {
                return Err(usage_of(w, "command"));
            }
            if a.has('o') && !allowed(sender, level, "citizens.admin") {
                return Err(err(w, M_NO_PERM, &[]));
            }
            if command.starts_with("npc select") {
                return Err(err(w, "npc select not currently supported within commands. Use --id <id> instead", &[]));
            }
            let mut hand = if a.has('l') && a.has('r') {
                Hand::Both
            } else if a.has('l') {
                Hand::Left
            } else {
                Hand::Right
            };
            if a.has('s') && hand != Hand::Both {
                hand = if hand == Hand::Left { Hand::ShiftLeft } else { Hand::ShiftRight };
            }
            let mut spec = CommandSpec::new(&command, hand);
            spec.player = a.has('p') || a.has('o');
            spec.op = a.has('o');
            spec.npc = a.has('n');
            if let Some(p) = a.value("permissions").or_else(|| a.value("permission")) {
                spec.permissions = p.split(',').map(str::to_owned).collect();
            }
            if let Some(v) = a.value("cooldown") {
                spec.cooldown = parse_i64(w, v)?;
            }
            if let Some(v) = a.value("gcooldown") {
                spec.global_cooldown = parse_i64(w, v)?;
            }
            if let Some(v) = a.value("delay") {
                spec.delay = parse_i64(w, v)?;
            }
            if let Some(v) = a.value("n") {
                spec.n = parse_i64(w, v)?;
            }
            if let Some(v) = a.value("gn") {
                spec.gn = parse_i64(w, v)?;
            }
            if let Some(v) = a.value("cost") {
                spec.cost = v.parse::<f64>().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
            }
            if let Some(v) = a.value("expcost") {
                spec.experience_cost = parse_i64(w, v)?;
            }
            let key = npc_mut(w, id)?.add_command_spec(&spec);
            w.mark_dirty();
            let mut out = ok(w, M_CMD_ADDED, &[&command, &key]);
            let unenforced = w
                .saves
                .get(id)
                .map(NpcData::commands)
                .and_then(|c| c.into_iter().find(|c| c.key == key))
                .and_then(|c| c.unenforced);
            if let Some(why) = unenforced {
                out.extend(err(w, M_UNSUPPORTED, &[&format!("The command's {why} limit")]));
            }
            Ok(out)
        }
        "remove" => {
            let Some(target_arg) = rest.split_whitespace().next() else {
                return Err(usage_of(w, "command"));
            };
            if target_arg.eq_ignore_ascii_case("all") {
                npc_mut(w, id)?.clear_commands();
                w.mark_dirty();
                return Ok(ok(w, M_CMD_CLEARED, &[&name]));
            }
            let key = target_arg.parse::<i64>().unwrap_or(-1).to_string();
            let removed = w.saves.get_mut(id).is_some_and(|n| n.remove_command(&key));
            if !removed {
                return Err(err(w, M_CMD_UNKNOWN_ID, &[&key]));
            }
            w.mark_dirty();
            Ok(ok(w, M_CMD_REMOVED, &[&key]))
        }
        "execute" | "forgetplayer" | "clearerror" | "sequential" | "cycle" | "random" | "permissions" | "perms"
        | "cost" | "expcost" | "itemcost" | "hideerrors" | "errormessage" | "rememberlastused" => {
            Err(err(w, M_UNSUPPORTED, &[&format!("command {op}")]))
        }
        _ => Err(usage_of(w, "command")),
    }
}

// ---- editors of Citizens that are plain sub-commands here ------------------------------------

/// `/npc text`: the lines an NPC says when right-clicked (Citizens edits them in chat).
fn text_lines(w: &mut World, sender: &Sender, tail: &str) -> Res {
    let id = target(w, sender)?;
    let (op, rest) = tail.trim().split_once(' ').unwrap_or((tail.trim(), ""));
    let mut lines = npc_mut(w, id)?.talk_lines();
    let reply = match op {
        "" | "list" => {
            let mut out = say(w, &header(&format!("Text ({} lines)", lines.len())));
            for (i, l) in lines.iter().enumerate() {
                out.extend(say(w, &format!("[[{i}]] {l}")));
            }
            return Ok(out);
        }
        "add" if !rest.trim().is_empty() => {
            lines.push(rest.trim().to_owned());
            say(w, "Line added.")
        }
        "remove" => {
            let i: usize = rest.trim().parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
            if i >= lines.len() {
                return Err(err(w, M_NO_SUCH_LINE, &[]));
            }
            lines.remove(i);
            say(w, &format!("Line [[{i}]] removed."))
        }
        "clear" => {
            lines.clear();
            say(w, "Text cleared.")
        }
        _ => return Err(err(w, "Usage: /npc text add <text>|remove <line>|clear|list", &[])),
    };
    npc_mut(w, id)?.set_talk_lines(&lines);
    w.mark_dirty();
    Ok(reply)
}

/// An item id as the game spells it, or none when it cannot be one.
fn item_id(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let plain = lower.trim_start_matches("minecraft:");
    (!plain.is_empty() && plain.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .then(|| plain.to_owned())
}

/// `/npc equip <slot> [item] [amount]`: an empty slot when no item is given (Citizens opens a GUI).
fn equip(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let usage = "Usage: /npc equip <hand|off_hand|helmet|chestplate|leggings|boots> [item] [amount]";
    let slot = args.words.first().ok_or_else(|| err(w, usage, &[]))?.to_ascii_lowercase();
    if !crate::npc::EQUIPMENT_SLOTS.iter().any(|(c, _)| *c == slot) {
        return Err(err(w, usage, &[]));
    }
    let item = args.words.get(1).filter(|s| !s.eq_ignore_ascii_case("air"));
    let item = match item {
        Some(i) => Some(item_id(i).ok_or_else(|| err(w, "Unknown item {0}.", &[i]))?),
        None => None,
    };
    let count = args.words.get(2).and_then(|c| c.parse::<i64>().ok()).unwrap_or(1).clamp(1, 64);
    npc_mut(w, id)?.set_equipment(&slot, item.as_deref().map(|i| (i, count)));
    let shown = item.unwrap_or_else(|| "nothing".to_owned());
    w.mark_dirty();
    w.respawn(id);
    Ok(say(w, &format!("{slot} set to [[{shown}]].")))
}

/// `/npc setequipment [slot] [item]`, with Bukkit's slot names.
fn setequipment(w: &mut World, sender: &Sender, args: &Args, c: &Cmd) -> Res {
    let id = target(w, sender)?;
    if args.has('c') {
        return Err(err(w, M_UNSUPPORTED, &["-c"]));
    }
    let slot_arg = args.words.first().cloned().unwrap_or_default().to_ascii_uppercase();
    let slot = match slot_arg.as_str() {
        "HAND" => "hand",
        "OFF_HAND" => "off_hand",
        "HEAD" => "helmet",
        "CHEST" => "chestplate",
        "LEGS" => "leggings",
        "FEET" => "boots",
        _ => return Err(usage_error(w, None, c)),
    };
    let item = match args.words.get(1) {
        None => None,
        Some(i) if i.eq_ignore_ascii_case("hand") => {
            return Err(err(w, M_UNSUPPORTED, &["Taking the item in your hand"]));
        }
        Some(i) => Some(item_id(i).ok_or_else(|| err(w, "Unknown item {0}.", &[i]))?),
    };
    npc_mut(w, id)?.set_equipment(slot, item.as_deref().map(|i| (i, 1)));
    w.mark_dirty();
    w.respawn(id);
    // Citizens prints the Bukkit ItemStack as it is.
    let shown = item.map_or_else(|| "null".to_owned(), |i| format!("ItemStack{{{} x 1}}", i.to_ascii_uppercase()));
    Ok(ok(w, M_EQUIPMENT_SET, &[&slot_arg, &shown]))
}

/// `/npc path add|remove <point>|clear|list`: the linear waypoints the NPC walks in order (no
/// pathfinding; Citizens' path-recording editor is not done).
fn path(w: &mut World, sender: &Sender, tail: &str) -> Res {
    let id = target(w, sender)?;
    let (op, rest) = tail.trim().split_once(' ').unwrap_or((tail.trim(), ""));
    match op {
        "add" => {
            let loc = here(w, sender)?;
            npc_mut(w, id)?.add_waypoint(&loc);
            w.mark_dirty();
            Ok(say(w, &format!("Point added at [[{:.1} {:.1} {:.1}]].", loc.x, loc.y, loc.z)))
        }
        "" | "list" => {
            let points = npc_mut(w, id)?.waypoints();
            let mut out = say(w, &header(&format!("Path ({} points)", points.len())));
            for (i, p) in points.iter().enumerate() {
                out.extend(say(w, &format!("[[{i}]] {} {:.1} {:.1} {:.1}", p.world, p.x, p.y, p.z)));
            }
            Ok(out)
        }
        "remove" => {
            let i: usize = rest.trim().parse().map_err(|_| err(w, M_INVALID_NUMBER, &[]))?;
            if !npc_mut(w, id)?.remove_waypoint(i) {
                return Err(err(w, "No such point.", &[]));
            }
            w.mark_dirty();
            Ok(say(w, &format!("Point [[{i}]] removed.")))
        }
        "clear" => {
            npc_mut(w, id)?.clear_waypoints();
            w.mark_dirty();
            Ok(say(w, "Path cleared."))
        }
        _ => Err(err(w, "Usage: /npc path add|remove <point>|clear|list", &[])),
    }
}

// ---- /trait -----------------------------------------------------------------------------------

/// Traits `/trait` and `create --trait` know. Citizens asks its trait factory; these are the ones
/// this port (or a mod built on it, Sentinel) reads from `traitnames`. The NPC's own basics
/// (`type`, `location`, `spawned`, `owner`, the skin) are not listed: removing them would break it.
pub const KNOWN_TRAITS: &[&str] =
    &["sentinel", "lookclose", "commandtrait", "hologramtrait", "waypoints", "age", "gravity", "gamemodetrait"];

fn trait_cmd(w: &mut World, sender: &Sender, rest: &str) -> Res {
    let id = target(w, sender)?;
    let rest = rest.trim();
    let (first, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let (mode, list) = match first.to_ascii_lowercase().as_str() {
        "" => return Err(trait_usage(w, "[trait name], [trait name]...")),
        "add" | "a" if !tail.trim().is_empty() => ('a', tail),
        "remove" | "rem" | "r" if !tail.trim().is_empty() => ('r', tail),
        "add" | "a" => return Err(trait_usage(w, "add [trait name]...")),
        "remove" | "rem" | "r" => return Err(trait_usage(w, "remove [trait names]...")),
        "list" => {
            let names = w.saves.get(id).map(NpcData::trait_names).unwrap_or_default();
            let mut out = say(w, &header(&format!("{} traits", name_at(w, id))));
            for t in names {
                out.extend(say(w, &format!("     - [[{t}]]")));
            }
            return Ok(out);
        }
        "clearsaves" => return Err(err(w, M_UNSUPPORTED, &["trait clearsaves"])),
        _ => ('t', rest),
    };
    let level = w.cfg.command_level;
    let (mut added, mut removed, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    for raw in list.split(',') {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if !(exact(sender, level, &format!("citizens.npc.trait.{name}")) || exact(sender, level, "citizens.npc.trait.*")) {
            failed.push(format!("{name}: No permission"));
            continue;
        }
        if !KNOWN_TRAITS.contains(&lower.as_str()) {
            failed.push(format!("{name}: Trait not found"));
            continue;
        }
        let has = w.saves.get(id).is_some_and(|n| n.has_trait(&lower));
        match (mode, has) {
            ('a', true) => failed.push(format!("{name}: Already added")),
            ('r', false) => failed.push(format!("{name}: Trait not attached")),
            ('r', true) | ('t', true) => {
                npc_mut(w, id)?.remove_trait(&lower);
                removed.push(format!("[[{name}]]"));
            }
            _ => {
                npc_mut(w, id)?.add_trait_name(&lower);
                added.push(format!("[[{name}]]"));
            }
        }
    }
    let mut out = Vec::new();
    if !added.is_empty() || !removed.is_empty() {
        w.mark_dirty();
    }
    if !added.is_empty() {
        out.extend(ok(w, M_TRAITS_ADDED, &[&added.join(", ")]));
    }
    if !removed.is_empty() {
        out.extend(ok(w, M_TRAITS_REMOVED, &[&removed.join(", ")]));
    }
    if !failed.is_empty() {
        let t = if mode == 'r' { M_TRAITS_FAILED_TO_REMOVE } else { M_TRAITS_FAILED_TO_ADD };
        out.extend(err(w, t, &[&failed.join(", ")]));
    }
    Ok(out)
}

fn trait_usage(w: &World, usage: &str) -> Reply {
    let mut out = err(w, M_TOO_FEW, &[]);
    out.push(w.cfg.messages.error(&format!("/trait {usage}")));
    out
}

// ---- debug, gamemode, flyable ---------------------------------------------------------------

fn debug(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    if args.has('p') {
        let on = !w.saves.get(id).and_then(|n| n.map.at(&["navigator", "debug"])).is_some_and(|v| v.as_bool() == Some(true));
        npc_mut(w, id)?.map.put(&["navigator", "debug"], Node::bool(on));
        w.mark_dirty();
        return Ok(say(w, &format!("Path debugging set to {on}")));
    }
    if args.has('n') {
        let speed = w.saves.get(id).map_or(1.0, NpcData::speed_modifier);
        return Ok(say(
            w,
            &format!("Pathfinder type [[STRAIGHT_LINE]] (this port walks straight at the target)<br>Speed [[{}]]<br>", jnum(speed)),
        ));
    }
    if args.has('i') {
        if sender.uuid.is_none() {
            return Err(err(w, M_MUST_BE_INGAME, &[]));
        }
        return Err(err(w, M_UNSUPPORTED, &["debug -i"]));
    }
    Ok(Vec::new())
}

fn gamemode(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target_of_kind(w, sender, &["PLAYER"])?;
    let name = name_at(w, id);
    let current = w
        .saves
        .get(id)
        .and_then(|n| n.map.at(&["traits", "gamemodetrait", "gamemode"]))
        .and_then(|v| v.text())
        .unwrap_or_else(|| "SURVIVAL".into());
    let Some(word) = args.words.first() else {
        return Ok(ok(w, M_GAMEMODE_DESCRIBE, &[&name, &pretty_enum(&current)]));
    };
    let mode = word.to_ascii_uppercase();
    if !["SURVIVAL", "CREATIVE", "ADVENTURE", "SPECTATOR"].contains(&mode.as_str()) {
        return Err(err(w, M_GAMEMODE_INVALID, &[word]));
    }
    let npc = npc_mut(w, id)?;
    npc.map.put(&["traits", "gamemodetrait", "gamemode"], Node::str(&mode));
    npc.add_trait_name("gamemodetrait");
    w.mark_dirty();
    Ok(ok(w, M_GAMEMODE_SET, &[&pretty_enum(&mode)]))
}

fn flyable(w: &mut World, sender: &Sender, args: &Args) -> Res {
    let id = target(w, sender)?;
    let kind = w.saves.get(id).map(NpcData::kind).unwrap_or_default();
    let always = ["VEX", "PARROT", "ALLAY", "BEE", "PHANTOM", "BREEZE", "BAT", "BLAZE", "ENDER_DRAGON", "WITHER"];
    if always.contains(&kind.as_str()) || kind.contains("GHAST") {
        return Err(err(w, M_REQUIREMENTS_INVALID_MOB_TYPE, &[&pretty_enum(&kind)]));
    }
    let current = w.saves.get(id).and_then(|n| n.map.at(&["metadata", "flyable"])).is_some_and(|v| v.as_bool() == Some(true));
    let on = match args.words.first().map(|s| s.to_ascii_lowercase()) {
        Some(v) if v == "true" => true,
        Some(v) if v == "false" => false,
        Some(_) => return Err(usage_of(w, "flyable")),
        None => !current,
    };
    npc_mut(w, id)?.map.put(&["metadata", "flyable"], Node::bool(on));
    w.mark_dirty();
    let name = name_at(w, id);
    Ok(ok(w, if on { M_FLYABLE_SET } else { M_FLYABLE_UNSET }, &[&name]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::world::Op;

    fn sender(level: u8) -> Sender {
        Sender {
            uuid: Some(0xA1),
            name: "Alex".into(),
            permission: level,
            grants: Vec::new(),
            at: Some((
                "minecraft:overworld".into(),
                Location {
                    world: "world".into(),
                    x: 10.0,
                    y: 64.0,
                    z: 10.0,
                    yaw: 45.0,
                    pitch: 0.0,
                    body_yaw: 45.0,
                },
            )),
        }
    }

    fn world() -> World {
        let mut cfg = Config::default();
        cfg.skins.fetch = false;
        World::new(cfg, Saves::empty(), None)
    }

    fn run_ok(w: &mut World, s: &Sender, line: &str) -> Vec<String> {
        run(w, s, line, || Ok(Saves::empty()))
    }

    #[test]
    fn flags() {
        let a = Args::parse("Bob the NPC --type villager -lp --at=1:2:3");
        assert_eq!(a.words, vec!["Bob", "the", "NPC"]);
        assert_eq!(a.value("type"), Some("villager"));
        assert_eq!(a.value("at"), Some("1:2:3"));
        assert!(a.has('l') && a.has('p'));
        assert_eq!(Args::parse("-5").words, vec!["-5"]);
    }

    #[test]
    fn messages_render_like_citizens() {
        let w = world();
        assert_eq!(ok(&w, M_RENAMED, &["Bob", "Al"]), vec!["&aYou renamed &eBob&a to &eAl&a."]);
        // `''` is a quote, a stray `]]` goes back to the message colour.
        assert_eq!(ok(&w, M_OWNER, &["Bob", "x"]), vec!["&a&eBob&a's owner is &ex&a."]);
        assert_eq!(
            ok(&w, M_GLOWING_COLOR_SET, &["Bob", "&cred"]),
            vec!["&a&eBob&a's glowing color set to &cred&a."]
        );
        // End tags go back to the message colour, `<br>` breaks a line, click tags vanish.
        assert_eq!(say(&w, &header("Bob")), vec!["&a&e=====[&a Bob&e ]====="]);
        assert_eq!(ok(&w, M_SELECTION_PROMPT, &[]).len(), 2);
        assert_eq!(
            ok(&w, M_REMOVED, &["Bob", "3"]),
            vec!["&aYou permanently removed &eBob&a ID &e3&a (&c&nundo&r&a&a)."]
        );
        assert_eq!(err(&w, M_MUST_SELECT, &[]), vec!["&cYou must have an NPC selected to execute that command."]);
    }

    #[test]
    fn create_select_and_edit() {
        let mut w = world();
        let s = sender(2);
        let r = run_ok(&mut w, &s, "npc create &6Bob Guide");
        assert!(r[0].contains("Created") && r[0].contains("Bob Guide"), "{r:?}");
        assert_eq!(w.selected(0xA1), Some(0));
        let npc = w.saves.get(0).unwrap();
        assert_eq!(npc.location().unwrap().x, 10.0);
        assert_eq!(npc.owner(), Some(0xA1));
        let r = run_ok(&mut w, &s, "npc lookclose");
        assert!(w.saves.get(0).unwrap().look_close());
        assert!(r[0].contains("will now rotate when players are nearby"), "{r:?}");
        let r = run_ok(&mut w, &s, "npc lookclose --range 5");
        assert_eq!(w.saves.get(0).unwrap().look_range(), Some(5.0));
        assert!(r[0].contains("range set to") && r[0].contains("5.0"), "{r:?}");
        assert!(w.saves.get(0).unwrap().look_close(), "a range does not toggle");
        run_ok(&mut w, &s, "npc look --randompitchrange -5,5");
        assert_eq!(w.saves.get(0).unwrap().random_ranges().1, (-5.0, 5.0));
        let r = run_ok(&mut w, &s, "npc vulnerable");
        assert!(!w.saves.get(0).unwrap().protected());
        assert!(r[0].contains("is now vulnerable"), "{r:?}");
        run_ok(&mut w, &s, "npc glowing --color GOLD");
        assert_eq!(w.saves.get(0).unwrap().team_color().as_deref(), Some("GOLD"));
        assert!(w.saves.get(0).unwrap().glowing());
        let r = run_ok(&mut w, &s, "npc name -h");
        assert_eq!(w.saves.get(0).unwrap().nameplate(), Nameplate::Hover);
        assert!(r[0].contains("Nameplate visibility set to"), "{r:?}");
        run_ok(&mut w, &s, "npc name");
        assert_eq!(w.saves.get(0).unwrap().nameplate(), Nameplate::Shown, "hover toggles to shown");
        run_ok(&mut w, &s, "npc name");
        assert_eq!(w.saves.get(0).unwrap().nameplate(), Nameplate::Hidden);
        run_ok(&mut w, &s, "npc hologram add &a&lOPEN");
        run_ok(&mut w, &s, "npc hologram set 0 &c&lCLOSED");
        assert_eq!(w.saves.get(0).unwrap().hologram_lines()[0].text, "&c&lCLOSED");
        run_ok(&mut w, &s, "npc hologram insert bottom first");
        assert_eq!(w.saves.get(0).unwrap().hologram_lines()[0].text, "first");
        run_ok(&mut w, &s, "npc hologram margintop top 0.5");
        assert_eq!(w.saves.get(0).unwrap().hologram_lines()[1].margin_top, 0.5);
        let r = run_ok(&mut w, &s, "npc hologram remove 7");
        assert!(r[0].contains("Invalid line number."), "{r:?}");
        run_ok(&mut w, &s, "npc command add -r -p server lobby");
        let c = &w.saves.get(0).unwrap().commands()[0];
        assert!(c.player && c.hand == Hand::Right && c.command == "server lobby");
        run_ok(&mut w, &s, "npc command add say hi");
        assert_eq!(w.saves.get(0).unwrap().commands()[1].hand, Hand::Right, "right click is the default");
        run_ok(&mut w, &s, "npc command add -l -r both");
        assert_eq!(w.saves.get(0).unwrap().commands()[2].hand, Hand::Both);
        let r = run_ok(&mut w, &s, "npc cmd remove 1");
        assert!(r[0].contains("Command") && r[0].contains("removed"), "{r:?}");
        assert_eq!(w.saves.get(0).unwrap().commands().len(), 2);
        run_ok(&mut w, &s, "npc command add again");
        assert_eq!(w.saves.get(0).unwrap().commands()[2].key, "1", "the lowest free id is reused");
        let r = run_ok(&mut w, &s, "npc rename Alice");
        assert_eq!(w.saves.get(0).unwrap().name(), "Alice");
        assert!(r[0].contains("renamed"), "{r:?}");
        run_ok(&mut w, &s, "npc skin Steve");
        assert_eq!(w.saves.get(0).unwrap().skin().name.as_deref(), Some("Steve"));
        let out = run_ok(&mut w, &s, "npc list");
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].contains("NPCs 1/1"), "{out:?}");
    }

    #[test]
    fn needs_the_level() {
        let mut w = world();
        let r = run_ok(&mut w, &sender(0), "npc create Bob");
        assert!(r[0].starts_with("&c"));
        assert!(r[0].contains("You don't have permission to execute that command."), "{r:?}");
        assert!(w.saves.npcs.is_empty());
    }

    #[test]
    fn argument_counts_and_unknown_words() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create A");
        let r = run_ok(&mut w, &s, "npc id extra");
        assert_eq!(r[0], "&cToo many arguments.");
        assert_eq!(r[1], "&c/npc id");
        let r = run_ok(&mut w, &s, "npc rename");
        assert_eq!(r[0], "&cToo few arguments.");
        let r = run_ok(&mut w, &s, "npc nonsense");
        assert_eq!(r[0], "&cToo many arguments.");
        assert_eq!(run_ok(&mut w, &s, "npc id"), vec!["0".to_string()]);
    }

    #[test]
    fn selection_rules() {
        let mut w = world();
        let s = sender(2);
        let r = run_ok(&mut w, &s, "npc tphere");
        assert!(r[0].contains("You must have an NPC selected"), "{r:?}");
        run_ok(&mut w, &s, "npc create A --at 0:64:0:world");
        run_ok(&mut w, &s, "npc create B --at 12:64:12:world");
        run_ok(&mut w, &s, "npc select 0");
        assert_eq!(w.selected(0xA1), Some(0));
        let r = run_ok(&mut w, &s, "npc select 0");
        assert!(r[0].contains("already have that NPC selected"), "{r:?}");
        run_ok(&mut w, &s, "npc sel");
        assert_eq!(w.selected(0xA1), Some(1), "B is nearest to 10/64/10");
        let r = run_ok(&mut w, &s, "npc select 99");
        assert!(r[0].contains("No NPC could be found."), "{r:?}");
        assert!(run_ok(&mut w, &s, "npc select Nobody").is_empty(), "a name no NPC has is silent");
        run_ok(&mut w, &s, "npc select a");
        assert_eq!(w.selected(0xA1), Some(0), "names match without case");
        let r = run_ok(&mut w, &s, "npc desel");
        assert_eq!(r, vec!["&aDeselected NPC."]);
        assert_eq!(w.selected(0xA1), None);
    }

    #[test]
    fn owners_only() {
        let mut w = world();
        let mut other = sender(0);
        other.uuid = Some(0xB2);
        other.grants = vec![
            "citizens.npc.create".into(),
            "citizens.npc.create.player".into(),
            "citizens.npc.select".into(),
            "citizens.npc.rename".into(),
        ];
        run_ok(&mut w, &sender(2), "npc create Mine");
        run_ok(&mut w, &other, "npc select 0");
        let r = run_ok(&mut w, &other, "npc rename Stolen");
        assert!(r[0].contains("You must be the owner of this NPC"), "{r:?}");
        assert_eq!(w.saves.get(0).unwrap().name(), "Mine");
        run_ok(&mut w, &other, "npc create Own");
        run_ok(&mut w, &other, "npc rename Renamed");
        assert_eq!(w.saves.get(1).unwrap().name(), "Renamed");
    }

    #[test]
    fn spawn_despawn_remove() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create A");
        w.take_ops();
        let r = run_ok(&mut w, &s, "npc despawn");
        assert!(r[0].contains("You despawned"), "{r:?}");
        assert!(!w.saves.get(0).unwrap().spawned());
        let r = run_ok(&mut w, &s, "npc spawn 0");
        assert!(r[0].contains("You spawned"), "{r:?}");
        assert!(w.saves.get(0).unwrap().spawned());
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Console(c) if c.contains("summon"))));
        let r = run_ok(&mut w, &s, "npc spawn 0");
        assert!(r[0].contains("already spawned"), "{r:?}");
        let r = run_ok(&mut w, &s, "npc remove");
        assert!(r[0].contains("You permanently removed"), "{r:?}");
        assert!(w.saves.npcs.is_empty());
        run_ok(&mut w, &s, "npc create B");
        run_ok(&mut w, &s, "npc create C");
        assert!(run_ok(&mut w, &sender(0), "npc remove all")[0].contains("permission"));
        run_ok(&mut w, &sender(4), "npc remove all");
        assert!(w.saves.npcs.is_empty());
    }

    #[test]
    fn tphere_and_moveto() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create A --at 0:64:0");
        let r = run_ok(&mut w, &s, "npc tphere");
        assert_eq!(w.saves.get(0).unwrap().location().unwrap().x, 10.0);
        assert!(r[0].contains("was teleported to world at 10, 64, 10 (45, 0)"), "{r:?}");
        run_ok(&mut w, &s, "npc moveto 1 70 2");
        let l = w.saves.get(0).unwrap().location().unwrap();
        assert_eq!((l.x, l.y, l.z), (1.0, 70.0, 2.0));
        run_ok(&mut w, &s, "npc moveto 3:71:4:world");
        assert_eq!(w.saves.get(0).unwrap().location().unwrap().x, 3.0);
        let r = run_ok(&mut w, &s, "npc moveto 1 2");
        assert!(r[0].contains("Format is x:y:z(:world) or x y z( world)."), "{r:?}");
        let r = run_ok(&mut w, &s, "npc moveto 1 2 3 nowhere");
        assert!(r[0].contains("World not found."), "{r:?}");
    }

    #[test]
    fn tp_goes_in_front_of_the_npc() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create A --at 0:64:0");
        w.take_ops();
        run_ok(&mut w, &s, "npc tp");
        let ops = w.take_ops();
        let Some(Op::Console(c)) = ops.first() else { panic!("{ops:?}") };
        // The NPC faces yaw 0 (south, +z): one block in front of it, looking back (yaw 180).
        assert!(c.contains(" 0 64 1 180 0"), "{c}");
        run_ok(&mut w, &s, "npc tp -e");
        let ops = w.take_ops();
        let Some(Op::Console(c)) = ops.first() else { panic!("{ops:?}") };
        assert!(c.contains(" 0 64 0 0 0"), "{c}");
    }

    #[test]
    fn citizens_reload_asks_twice() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create A");
        let r = run(&mut w, &s, "citizens reload", || Ok(Saves::empty()));
        assert!(r[0].contains("WARNING"), "{r:?}");
        assert_eq!(w.saves.npcs.len(), 1);
        let r = run(&mut w, &s, "citizens reload", || Ok(Saves::empty()));
        assert!(r.iter().any(|l| l.contains("Citizens reloaded.")), "{r:?}");
        assert!(w.saves.npcs.is_empty());
        let r = run(&mut w, &s, "citizens save", || Ok(Saves::empty()));
        assert!(r[0].contains("Saving Citizens"), "{r:?}");
        assert!(r.iter().any(|l| l.contains("Citizens saved.")), "{r:?}");
        assert!(run(&mut w, &sender(0), "citizens", || Ok(Saves::empty()))[0].contains("permission"));
    }

    #[test]
    fn every_usage_has_a_listed_node() {
        for u in usages() {
            assert!(NODES.contains(&node_of_usage(&u)), "{u}");
        }
    }

    #[test]
    fn aliases_complete_too() {
        let all = usages();
        for want in ["npc sel", "npc rem all", "npc desel", "npc tph", "npc move", "npc cmd add -l <command:text>", "npc look"] {
            assert!(all.iter().any(|u| u.starts_with(want)), "{want}");
        }
    }

    #[test]
    fn help_shows_no_argument_kinds() {
        for line in help_lines() {
            for w in line.split_whitespace() {
                assert!(!((w.starts_with('<') || w.starts_with('[')) && w.contains(':') && w.ends_with(']')), "{line}");
            }
        }
    }

    /// Horizon's usage rules: a literal starts the usage, literals are lower case, an optional
    /// argument is never followed by a required one, and `text` is last.
    #[test]
    fn usages_read_as_horizon_reads_them() {
        for u in usages() {
            let words: Vec<&str> = u.split_whitespace().collect();
            assert!(!words.is_empty() && !words[0].starts_with('<') && !words[0].starts_with('['), "{u}");
            let (mut optional, mut text) = (false, false);
            for w in words {
                assert!(!text, "text must be last: {u}");
                if w.starts_with('<') || w.starts_with('[') {
                    let is_optional = w.starts_with('[');
                    assert!(is_optional || !optional, "{u}");
                    optional |= is_optional;
                    text |= w.ends_with(":text>") || w.ends_with(":text]");
                } else {
                    assert!(!optional, "{u}");
                    assert!(w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c)), "{u}");
                }
            }
        }
    }

    #[test]
    fn a_granted_node_lets_a_non_operator_in() {
        let mut w = world();
        let mut s = sender(0);
        assert!(run_ok(&mut w, &s, "npc create Bob")[0].starts_with("&c"));
        s.grants = vec!["citizens.npc.create".into()];
        // The node for the command is not enough: the type has its own.
        assert!(run_ok(&mut w, &s, "npc create Bob")[0].contains("permission"));
        s.grants.push("citizens.npc.create.player".into());
        assert!(run_ok(&mut w, &s, "npc create Bob")[0].contains("Bob"));
        // Removing an NPC needs citizens.npc.remove (or citizens.admin), which was not granted.
        assert!(run_ok(&mut w, &s, "npc remove")[0].contains("permission"));
        s.grants.push("citizens.npc.remove".into());
        assert!(run_ok(&mut w, &s, "npc remove")[0].contains("You permanently removed"));
    }

    #[test]
    fn create_flags() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create -u -s Quiet --nameplate hover");
        let n = w.saves.get(0).unwrap();
        assert!(!n.spawned());
        assert_eq!(n.nameplate(), Nameplate::Hover);
        assert!(n.map.at(&["metadata", "silent"]).is_some());
        let r = run_ok(&mut w, &s, "npc create 12345678901234567");
        assert!(r[0].contains("cannot be longer than"), "{r:?}");
        assert_eq!(w.saves.get(1).unwrap().name(), "1234567890123456");
        let r = run_ok(&mut w, &s, "npc create X --type bad-type");
        assert!(r[0].contains("is not a valid mob type."), "{r:?}");
        run_ok(&mut w, &s, "npc create -c Mid --at 3.7:64:4.2:world");
        let l = w.saves.get(2).unwrap().location().unwrap();
        assert_eq!((l.x, l.z), (3.5, 4.5));
        assert!(run_ok(&mut w, &s, "npc create -b Baby")[0].contains("not supported"));
    }

    #[test]
    fn copy_owner_type_and_list() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Bob");
        let r = run_ok(&mut w, &s, "npc copy --name Twin");
        assert!(r[0].contains("has been copied."), "{r:?}");
        assert_eq!(w.saves.get(1).unwrap().name(), "Twin");
        assert_eq!(w.selected(0xA1), Some(1));
        let r = run_ok(&mut w, &s, "npc owner");
        assert!(r[0].contains("owner is"), "{r:?}");
        let r = run_ok(&mut w, &s, "npc owner server");
        assert!(r[0].contains("The server"), "{r:?}");
        assert_eq!(w.saves.get(1).unwrap().owner(), None);
        run_ok(&mut w, &s, "npc type villager");
        assert_eq!(w.saves.get(1).unwrap().kind(), "VILLAGER");
        assert!(run_ok(&mut w, &s, "npc type bad-one")[0].contains("is not a valid type."));
        // `list` shows what the sender owns; the console needs -a.
        let out = run_ok(&mut w, &s, "npc list");
        assert_eq!(out.len(), 2, "only Bob is Alex's now: {out:?}");
        let console = Sender { uuid: None, name: "Console".into(), permission: 4, grants: Vec::new(), at: None };
        let r = run_ok(&mut w, &console, "npc list"); assert!(r[0].contains("does not exist."), "{r:?}");
        assert_eq!(run_ok(&mut w, &console, "npc list -a").len(), 3);
        assert_eq!(run_ok(&mut w, &console, "npc list -a --type villager").len(), 2);
        assert!(run_ok(&mut w, &console, "npc list -a 5")[0].contains("does not exist."));
    }

    #[test]
    fn traits_that_show_in_the_entity() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Cow --type cow");
        let r = run_ok(&mut w, &s, "npc age baby");
        assert!(r[0].contains("is now a baby."), "{r:?}");
        assert!(run_ok(&mut w, &s, "npc age 5")[0].contains("Invalid age."));
        let ops = w.take_ops();
        assert!(ops.iter().any(|o| matches!(o, Op::Console(c) if c.contains("Age:-24000"))), "{ops:?}");
        assert!(run_ok(&mut w, &s, "npc powered")[0].contains("cannot be the mob type"));
        run_ok(&mut w, &s, "npc create Boom --type creeper");
        let r = run_ok(&mut w, &s, "npc powered");
        assert!(r[0].contains("will now be powered."), "{r:?}");
        run_ok(&mut w, &s, "npc create Wool --type sheep");
        run_ok(&mut w, &s, "npc sheep --color light_blue --sheared true");
        let ops = w.take_ops();
        assert!(ops.iter().any(|o| matches!(o, Op::Console(c) if c.contains("Color:3b") && c.contains("Sheared:1b"))), "{ops:?}");
        run_ok(&mut w, &s, "npc create Goo --type slime");
        let r = run_ok(&mut w, &s, "npc slimesize 4");
        assert!(r[0].contains("size set to"), "{r:?}");
        assert!(run_ok(&mut w, &s, "npc slimesize")[0].contains("size is"));
    }

    fn player(uuid: u128, name: &str, x: f64, z: f64) -> crate::look::PlayerView {
        crate::look::PlayerView {
            uuid,
            name: name.into(),
            dimension: "minecraft:overworld".into(),
            pos: [x, 64.0, z],
            game_mode: 0,
            permission: 0,
            entity_id: 1,
            sneaking: false,
        }
    }

    #[test]
    fn follow_pathto_speed_wander_walk() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Pal --at 0:64:0:world");
        w.online = vec![player(0xA1, "Alex", 30.0, 0.0)];
        let r = run_ok(&mut w, &s, "npc speed 2");
        assert!(r[0].contains("speed modifier set to") && r[0].contains("2.0"), "{r:?}");
        assert!(run_ok(&mut w, &s, "npc follow")[0].contains("is now following"));
        assert_eq!(w.saves.get(0).unwrap().follow_target(), Some(0xA1));
        let players = w.online.clone();
        for _ in 0..40 {
            w.tick(&players);
        }
        let x = w.current_location(0).unwrap().x;
        assert!(x > 10.0 && x < 30.0, "walked toward the player at 0.4 blocks a tick: {x}");
        for _ in 0..200 {
            w.tick(&players);
        }
        let x = w.current_location(0).unwrap().x;
        assert!((x - 28.0).abs() < 0.5, "stops 2 blocks short: {x}");
        assert!(run_ok(&mut w, &s, "npc follow --margin 5")[0].contains("blocks away from the target"));
        assert!(run_ok(&mut w, &s, "npc follow")[0].contains("no longer following anyone"));
        assert_eq!(w.saves.get(0).unwrap().follow_target(), None);
        // pathto walks in a straight line and says nothing.
        assert!(run_ok(&mut w, &s, "npc pathto 28 64 10").is_empty());
        for _ in 0..100 {
            w.tick(&players);
        }
        assert_eq!(w.current_location(0).unwrap().z, 10.0);
        // wander
        let r = run_ok(&mut w, &s, "npc wander");
        assert!(r[0].contains("Set the waypoint provider to"), "{r:?}");
        let r = run_ok(&mut w, &s, "npc wander xyrange 4 1");
        assert!(r[0].contains("Wander range set to"), "{r:?}");
        run_ok(&mut w, &s, "npc wander delay 10");
        assert_eq!(w.saves.get(0).unwrap().wander(), Some((4, 1, 10)));
        let before = w.current_location(0).unwrap();
        for _ in 0..400 {
            w.tick(&players);
        }
        let after = w.current_location(0).unwrap();
        assert!((after.x - before.x).hypot(after.z - before.z) > 0.0, "it wandered");
        assert!(run_ok(&mut w, &s, "npc wander")[0].contains("linear"));
    }

    #[test]
    fn rotate_pose_anchor_tpto_hurt_playsound() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Statue --at 0:64:0:world");
        w.online = vec![player(0xB2, "Bea", 5.0, 5.0)];
        w.take_ops();
        run_ok(&mut w, &s, "npc rotate --body 90 --pitch 10");
        let ops = w.take_ops();
        assert!(ops.iter().any(|o| matches!(o, Op::Console(c) if c.contains("rotate") && c.contains("90") && c.contains("10"))), "{ops:?}");
        run_ok(&mut w, &s, "npc rotate --toentity Bea");
        assert!(!w.take_ops().is_empty());
        // poses
        assert!(run_ok(&mut w, &s, "npc pose --save North --yaw 180 --pitch 0 -d")[0].contains("Pose added."));
        assert_eq!(w.saves.get(0).unwrap().string_list(&["traits", "poses", "list"]), vec!["north;0.0;180.0".to_string()]);
        assert!(run_ok(&mut w, &s, "npc pose --save north")[0].contains("already exists"));
        assert!(run_ok(&mut w, &s, "npc pose --assume south")[0].contains("does not exist"));
        w.take_ops();
        run_ok(&mut w, &s, "npc pose --assume north");
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Console(c) if c.contains("180"))));
        assert_eq!(w.saves.get(0).unwrap().default_pose(), Some((180.0, 0.0)));
        assert!(run_ok(&mut w, &s, "npc pose --remove north")[0].contains("Pose removed."));
        // anchors
        assert!(run_ok(&mut w, &s, "npc anchor --save home")[0].contains("Anchor added."));
        assert_eq!(w.saves.get(0).unwrap().string_list(&["traits", "anchors", "list"]), vec!["home;world;10.0;64.0;10.0".to_string()]);
        assert!(run_ok(&mut w, &s, "npc anchor --save HOME")[0].contains("already exists"));
        run_ok(&mut w, &s, "npc moveto 0 64 0");
        run_ok(&mut w, &s, "npc anchor --assume home");
        assert_eq!(w.current_location(0).unwrap().x, 10.0);
        assert_eq!(run_ok(&mut w, &s, "npc anchor").len(), 3);
        assert!(run_ok(&mut w, &s, "npc anchor --remove home")[0].contains("Anchor removed."));
        // tpto: the selected NPC to a player
        w.take_ops();
        assert!(run_ok(&mut w, &s, "npc tpto Bea")[0].contains("Teleported successfully."));
        assert_eq!(w.current_location(0).unwrap().x, 5.0);
        assert!(run_ok(&mut w, &s, "npc tpto Nobody")[0].contains("Destination entity not found."));
        // hurt (the NPC is protected until it is made vulnerable) and playsound
        assert!(run_ok(&mut w, &s, "npc hurt 3")[0].contains("protected"));
        run_ok(&mut w, &s, "npc vulnerable");
        w.take_ops();
        run_ok(&mut w, &s, "npc hurt 3");
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Console(c) if c.contains("damage") && c.ends_with(" 3"))));
        run_ok(&mut w, &s, "npc playsound entity.cow.ambient players 0.5 2");
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Console(c) if c.contains("playsound entity.cow.ambient player @a") && c.ends_with("0.5 2"))));
    }

    #[test]
    fn sounds_play_on_hurt() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Cow --type cow");
        assert!(run_ok(&mut w, &s, "npc sound")[0].contains("null"));
        let r = run_ok(&mut w, &s, "npc sound --hurt entity.cow.hurt");
        assert!(r[0].contains("entity.cow.hurt"), "{r:?}");
        w.on_entity_join("minecraft:overworld", 7, w.saves.get(0).unwrap().uuid(), true);
        w.take_ops();
        w.on_damaged(7, 1.0, "minecraft:generic", -1);
        assert!(w.take_ops().iter().any(|o| matches!(o, Op::Console(c) if c.contains("playsound entity.cow.hurt neutral @a"))));
        run_ok(&mut w, &s, "npc sound -n");
        assert!(w.saves.get(0).unwrap().sound("hurt").is_none());
    }

    #[test]
    fn talk_equip_path_and_owner() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Bob");
        run_ok(&mut w, &s, "npc text add Hello there");
        assert_eq!(w.saves.get(0).unwrap().talk_lines(), vec!["Hello there".to_string()]);
        run_ok(&mut w, &s, "npc equip hand diamond_sword");
        assert_eq!(w.saves.get(0).unwrap().equipment()[0].item, "diamond_sword");
        run_ok(&mut w, &s, "npc equip hand");
        assert!(w.saves.get(0).unwrap().equipment().is_empty());
        let r = run_ok(&mut w, &s, "npc setequipment head iron_helmet");
        assert_eq!(r, vec!["&aSet &eHEAD&a to &eItemStack{IRON_HELMET x 1}&a."]);
        assert_eq!(w.saves.get(0).unwrap().equipment()[0].item, "iron_helmet");
        run_ok(&mut w, &s, "npc path add");
        run_ok(&mut w, &s, "npc path add");
        assert_eq!(w.saves.get(0).unwrap().waypoints().len(), 2);
        run_ok(&mut w, &s, "npc path remove 0");
        assert_eq!(w.saves.get(0).unwrap().waypoints().len(), 1);
        let r = run_ok(&mut w, &s, "npc owner 00000000-0000-0000-0000-0000000000a1");
        assert!(r[0].contains("already the owner"), "{r:?}");
        assert!(run_ok(&mut w, &s, "npc equip nose stick")[0].starts_with("&c"));
    }

    #[test]
    fn trait_command_attaches_and_removes() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Bob");
        assert_eq!(run_ok(&mut w, &s, "trait sentinel"), vec!["&aAdded &esentinel&a successfully.".to_string()]);
        assert!(w.saves.get(0).unwrap().has_trait("sentinel"));
        assert!(run_ok(&mut w, &s, "trait add sentinel")[0].contains("Already added"));
        assert!(run_ok(&mut w, &s, "trait remove sentinel")[0].contains("Removed"));
        assert!(!w.saves.get(0).unwrap().has_trait("sentinel"));
        assert!(run_ok(&mut w, &s, "trait remove sentinel")[0].contains("Trait not attached"));
        assert!(run_ok(&mut w, &s, "trait nosuch")[0].contains("Trait not found"));
        run_ok(&mut w, &s, "npc create Al --trait sentinel,lookclose");
        assert!(w.saves.get(1).unwrap().has_trait("sentinel"));
        assert!(run_ok(&mut w, &s, "trait list").iter().any(|l| l.contains("sentinel")));
    }

    #[test]
    fn info_debug_gamemode_flyable() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Bob");
        assert!(run_ok(&mut w, &s, "npc info").iter().any(|l| l.contains("ID:")));
        assert!(run_ok(&mut w, &s, "npc debug -p")[0].contains("Path debugging set to true"));
        assert!(run_ok(&mut w, &s, "npc debug -p")[0].contains("false"));
        assert!(run_ok(&mut w, &s, "npc gamemode")[0].contains("survival"));
        assert!(run_ok(&mut w, &s, "npc gamemode creative")[0].contains("creative"));
        assert!(run_ok(&mut w, &s, "npc gamemode creative")[0].starts_with("&a"));
        assert!(run_ok(&mut w, &s, "npc gamemode nope")[0].contains("not a valid gamemode"));
        assert!(run_ok(&mut w, &s, "npc flyable")[0].contains("is now flyable"));
        assert!(run_ok(&mut w, &s, "npc flyable")[0].contains("no longer flyable"));
        // A real Citizens command this port lacks.
        assert!(run_ok(&mut w, &s, "npc aggressive true")[0].contains("not supported"));
        // Not a Citizens command at all.
        assert!(run_ok(&mut w, &s, "npc sneak")[0].contains("Too many arguments"));
    }

    #[test]
    fn owner_shows_names_and_pathto_walks() {
        let mut w = world();
        let s = sender(2);
        run_ok(&mut w, &s, "npc create Bob");
        w.profiles.insert(0xB2, "Notch".into());
        run_ok(&mut w, &s, "npc owner Notch");
        assert_eq!(w.saves.get(0).unwrap().owner(), Some(0xB2));
        assert!(run_ok(&mut w, &s, "npc owner")[0].contains("owner is &eNotch"));
        run_ok(&mut w, &s, "npc pathto 32 65 48");
        assert!(w.is_navigating(0) || !w.is_out(0));
    }
}
