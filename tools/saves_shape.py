# Prints the key structure of a Citizens saves.yml: key paths (NPC ids collapsed to <id>),
# value types and counts. Never prints values, except for a whitelist of enum-like keys.
import sys, collections, yaml
ENUM_KEYS = {"type", "lookclose.enabled", "protected", "nameplate-visible", "removefromplayerlist",
             "realtime", "enableRandomLook", "disablePacketHolograms", "headOnly", "linkedBody",
             "perPlayer", "targetNPCs", "useTargetLock", "rpd", "lookclose.range", "range"}
paths = collections.Counter(); enums = collections.defaultdict(collections.Counter)
def walk(node, path):
    if isinstance(node, dict):
        for k, v in node.items():
            k = str(k)
            p = path + "." + ("<id>" if path == "npc" else ("<n>" if k.isdigit() else k)) if path else k
            walk(v, p)
    elif isinstance(node, list):
        paths[path + "[]"] += 1
        for v in node: walk(v, path + "[]")
    else:
        paths[path + " :" + type(node).__name__] += 1
        leaf = path.split(".")[-1]
        if leaf in ENUM_KEYS or path.endswith("traits.type") or path.endswith("lookclose.enabled"):
            enums[path][str(node)] += 1
for f in sys.argv[1:]:
    with open(f) as fh: walk(yaml.safe_load(fh), "")
for p, n in sorted(paths.items()): print(f"{n:5d}  {p}")
print("--- enum-like values")
for p, c in sorted(enums.items()): print(p, dict(c))
