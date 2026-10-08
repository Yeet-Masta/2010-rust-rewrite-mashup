"""Extract the 26.3 creative tabs' item lists, in order, from the
decompiled `CreativeModeTabs`, `Items`, `BlockItemIds` and `ItemIds`.

usage: creative_tabs.py <mc-src> <datapack data/minecraft> <assets/minecraft> <out.json>
"""
import json
import os
import re
import sys

src, data, assets, out = sys.argv[1:5]
java = lambda path: open(os.path.join(src, "net/minecraft", path)).read()

COLORS = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray",
          "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"]
GAMEPLAY = ["white", "light_gray", "gray", "black", "brown", "red", "orange", "yellow",
            "lime", "green", "cyan", "light_blue", "blue", "purple", "magenta", "pink"]
STATES = ["", "exposed_", "weathered_", "oxidized_"]
WAXED = ["waxed_", "waxed_exposed_", "waxed_weathered_", "waxed_oxidized_"]


def collections(text, kind):
    """Constant -> plain id, ("color", {color: id}) or ("copper", (weathering, waxed))."""
    ids = {}
    for name, value in re.findall(r"public static final [\w<>]+ (\w+) = (.*?);\n", text, re.S):
        if m := re.match(r'(?:BlockItemId\.)?create\("([^"]+)"(?:, "([^"]+)")?\)', value):
            ids[name] = m.group(2) or m.group(1)
        elif m := re.match(r'createSimpleColored\("([^"]+)"\)', value):
            ids[name] = ("color", {c: f"{c}_{m.group(1)}" for c in COLORS})
        elif m := re.match(r'createSimpleCopper\("([^"]+)"\)', value):
            base = m.group(1)
            ids[name] = ("copper", ([p + base for p in STATES], [p + base for p in WAXED]))
        elif "COPPER_BLOCK_SPECIAL_NAMES" in value:
            special = ["copper_block", "copper", "copper", "copper"]
            ids[name] = ("copper", ([p + s for p, s in zip(STATES, special)],
                                    [p + s for p, s in zip(WAXED, special)]))
        elif m := re.match(r"create(\w+)\((\w+)\.(\w+)\)", value):
            ids[name] = (kind, m.group(1), m.group(3))
    return ids


block_ids = collections(java("references/BlockItemIds.java"), "block")
item_ids = collections(java("references/ItemIds.java"), "item")

pack_items = {f[:-5] for f in os.listdir(os.path.join(assets, "items"))}


def derived(entry):
    """`createSpawnEgg(EntityTypeIds.X)` and friends, resolved by the pack."""
    _, how, ref = entry
    path = ref.lower()
    source = {"MusicDisc": "world/item/JukeboxSongs.java",
              "PotterySherd": "world/level/block/entity/DecoratedPotPatterns.java",
              "ArmorTrimSmithingTemplate": "world/item/equipment/trim/TrimPatterns.java"}.get(how)
    if source and (m := re.search(rf'\b{ref} = \w+\("([^"]+)"\)', java(source))):
        path = m.group(1)
    guess = {
        "SpawnEgg": f"{path}_spawn_egg",
        "PotterySherd": f"{path}_pottery_sherd",
        "ArmorTrimSmithingTemplate": f"{path}_armor_trim_smithing_template",
        "MusicDisc": f"music_disc_{path}",
    }.get(how)
    if guess in pack_items:
        return guess
    # The constant and the registry path can differ (`THIRTEEN` vs `13`).
    raise SystemExit(f"unresolved {how}({ref})")


ITEMS = {}
items_java = java("world/item/Items.java")
for name, value in re.findall(r"public static final [\w<>]+ (\w+) = (.*?);\n", items_java, re.S):
    m = re.search(r"\b(BlockItemIds|ItemIds)\.(\w+)", value)
    if not m:
        continue
    table = block_ids if m.group(1) == "BlockItemIds" else item_ids
    entry = table.get(m.group(2))
    if entry is None:
        raise SystemExit(f"no id for {name} ({m.group(0)})")
    if isinstance(entry, tuple) and entry[0] in ("block", "item"):
        entry = derived(entry)
    ITEMS[name] = entry


def plain(name):
    entry = ITEMS[name]
    assert isinstance(entry, str), name
    return entry


def color_list(name, order):
    kind, table = ITEMS[name]
    assert kind == "color", name
    return [table[c] for c in order]


def copper(name):
    kind, (weathering, waxed) = ITEMS[name]
    assert kind == "copper", name
    return weathering, waxed


def stack(item, components=None):
    entry = {"id": f"minecraft:{item}"}
    if components:
        entry["components"] = components
    return entry


# Registries the generators read.
def potions():
    text = java("world/item/alchemy/Potions.java")
    ids = re.findall(r"register\(\s*PotionIds\.(\w+)", text)
    potion_ids = java("world/item/alchemy/PotionIds.java")
    paths = dict(re.findall(r'(\w+) = create\("([^"]+)"\)', potion_ids))
    return [paths.get(i, i.lower()) for i in ids]


def registry(folder):
    return sorted(f[:-5] for f in os.listdir(os.path.join(data, folder)) if f.endswith(".json"))


def tag(kind, name):
    values = json.load(open(os.path.join(data, "tags", kind, f"{name}.json")))["values"]
    out = []
    for value in values:
        if value.startswith("#"):
            out += tag(kind, value[1:].split(":")[1])
        else:
            out.append(value.split(":")[1])
    return out


def paintings(placeable):
    tagged = set(tag("painting_variant", "placeable"))
    found = []
    for name in registry("painting_variant"):
        if (name in tagged) == placeable:
            v = json.load(open(os.path.join(data, "painting_variant", f"{name}.json")))
            found.append((v["width"] * v["height"], v["width"], name))
    # `PAINTING_COMPARATOR`: area, then width; `sorted` is stable.
    found.sort(key=lambda p: (p[0], p[1]))
    return [stack("painting", {"minecraft:painting/variant": f"minecraft:{p[2]}"}) for p in found]


def enchanted_books(all_levels):
    books = []
    for name in registry("enchantment"):
        top = json.load(open(os.path.join(data, "enchantment", f"{name}.json")))["max_level"]
        for level in range(1, top + 1) if all_levels else [top]:
            books.append(stack("enchanted_book",
                               {"minecraft:stored_enchantments": {f"minecraft:{name}": level}}))
    return books


FLOWER_EFFECTS = {}
blocks_java = java("world/level/block/Blocks.java")
for name, body in re.findall(r"public static final Block (\w+) = register\((.*?)\);\n", blocks_java, re.S):
    if m := re.search(r"new (?:FlowerBlock|WitherRoseBlock)\(MobEffects\.(\w+), ([\d.]+)F", body):
        FLOWER_EFFECTS[name] = (m.group(1).lower(), int(float(m.group(2)) * 20.0 + 1e-6))
    elif m := re.search(r"EyeblossomBlock\.Type\.(\w+)", body):
        eye = java("world/level/block/EyeblossomBlock.java")
        t = re.search(m.group(1) + r"\((?:true|false), MobEffects\.(\w+), ([\d.]+)F", eye)
        FLOWER_EFFECTS[name] = (t.group(1).lower(), int(float(t.group(2)) * 20.0 + 1e-6))


def stews():
    seen, out = set(), []
    for name, value in re.findall(r"public static final Item (\w+) = (.*?);\n", items_java, re.S):
        block = re.search(r"Blocks\.(\w+)", value)
        if block and block.group(1) in FLOWER_EFFECTS:
            effect, duration = FLOWER_EFFECTS[block.group(1)]
            key = (effect, duration)
            if key in seen:
                continue
            seen.add(key)
            out.append(stack("suspicious_stew", {"minecraft:suspicious_stew_effects": [
                {"id": f"minecraft:{effect}", "duration": duration}]}))
    return out


OMINOUS_BANNER = stack(color_list("BANNER", ["white"])[0], {
    "minecraft:banner_patterns": [
        {"pattern": "minecraft:rhombus", "color": "cyan"},
        {"pattern": "minecraft:stripe_bottom", "color": "light_gray"},
        {"pattern": "minecraft:stripe_center", "color": "gray"},
        {"pattern": "minecraft:border", "color": "light_gray"},
        {"pattern": "minecraft:stripe_middle", "color": "black"},
        {"pattern": "minecraft:half_horizontal", "color": "light_gray"},
        {"pattern": "minecraft:circle", "color": "light_gray"},
        {"pattern": "minecraft:border", "color": "black"},
    ],
    "minecraft:tooltip_display": {"hidden_components": ["minecraft:banner_patterns"]},
    "minecraft:item_name": {"translate": "block.minecraft.ominous_banner"},
    "minecraft:rarity": "uncommon",
})


tabs_java = java("world/item/CreativeModeTabs.java")
TABS = []
for m in re.finditer(r"Registry\.register\(\s*registry,\s*(\w+),\s*CreativeModeTab\.builder\("
                     r"CreativeModeTab\.Row\.(\w+), (\d+)\)(.*?)\.build\(\)\s*\)", tabs_java, re.S):
    key, row, column, body = m.group(1), m.group(2), int(m.group(3)), m.group(4)
    tab = {
        "id": re.search(rf'{key} = createKey\("(\w+)"\)', tabs_java).group(1),
        "row": row.lower(),
        "column": column,
        "title": re.search(r'translatable\("([^"]+)"\)', body).group(1),
        "icon": None,
        "aligned_right": ".alignedRight()" in body,
        "show_title": ".hideTitle()" not in body,
        "scroll_bar": ".noScrollBar()" not in body,
        "type": (re.search(r"Type\.(\w+)", body) or re.match("(CATEGORY)", "CATEGORY")).group(1).lower(),
        "background": "items",
        "items": [],
        "search_only": [],
    }
    if m2 := re.search(r'backgroundTexture\((\w+)_BACKGROUND\)', body):
        tab["background"] = {"INVENTORY": "inventory", "SEARCH": "item_search"}[m2.group(1)]
    icon = re.search(r"icon\(\(\) -> new ItemStack\((\w+)\.(\w+)(?:\.(\w+)\(\))?\)\)", body)
    if icon.group(3):
        tab["icon"] = "minecraft:" + color_list(icon.group(2), [icon.group(3)])[0]
    else:
        tab["icon"] = "minecraft:" + plain(icon.group(2))
    items = tab["items"]
    for line in body.splitlines():
        line = line.strip()
        if a := re.fullmatch(r"\w+\.accept\(Items\.(\w+)\);", line):
            items.append(stack(plain(a.group(1))))
        elif a := re.fullmatch(r"\w+\.accept\(Items\.(\w+)\.(\w+)\(\)\);", line):
            items.append(stack(color_list(a.group(1), [a.group(2)])[0]))
        elif a := re.fullmatch(r"\w+\.accept\(Items\.(\w+)\.waxed\(\)\.unaffected\(\)\);", line):
            items.append(stack(copper(a.group(1))[1][0]))
        elif a := re.fullmatch(r"Items\.(\w+)\.forEach\(.*\);", line):
            entry = ITEMS[a.group(1)]
            if entry[0] == "color":
                items += [stack(i) for i in color_list(a.group(1), COLORS)]
            else:
                w, x = copper(a.group(1))
                items += [stack(i) for i in w + x]
        elif a := re.fullmatch(r"Items\.(\w+)\.waxed\(\)\.forEach\(.*\);", line):
            items += [stack(i) for i in copper(a.group(1))[1]]
        elif a := re.fullmatch(r"registerColoredItems\(\w+, gameplayColorOrder, Items\.(\w+)\);", line):
            items += [stack(i) for i in color_list(a.group(1), GAMEPLAY)]
        elif a := re.fullmatch(r"copperBlockFamilies\(family -> family\.(weathering|waxed)\(\)\.forEach.*\);", line):
            families = re.search(r"copperBlockFamilies\(final.*?\{(.*?)\n   \}", tabs_java, re.S).group(1)
            for f in re.findall(r"Items\.(\w+)", families):
                w, x = copper(f)
                items += [stack(i) for i in (w if a.group(1) == "weathering" else x)]
        elif "getOminousBannerInstance" in line:
            items.append(OMINOUS_BANNER)
        elif line.startswith("generateFireworksAllDurations"):
            items += [stack("firework_rocket", {"minecraft:fireworks": {"flight_duration": d}}) for d in (1, 2, 3)]
        elif line.startswith("generateSuspiciousStews"):
            items += stews()
        elif line.startswith("generateOminousBottles"):
            items += [stack("ominous_bottle", {"minecraft:ominous_bottle_amplifier": i}) for i in range(5)]
        elif a := re.search(r"generatePotionEffectTypes\(\s*\w+, potions, Items\.(\w+)", line):
            pass  # handled below, over the multi-line call
        elif line.startswith("generateEnchantmentBookTypesOnlyMaxLevel"):
            # `TabVisibility.PARENT_TAB_ONLY`: not in the search tab.
            items += [dict(book, tab_only=True) for book in enchanted_books(False)]
        elif line.startswith("generateEnchantmentBookTypesAllLevels"):
            tab["search_only"] += enchanted_books(True)
        elif "TestBlock.setModeOnStack" in line:
            items += [stack("test_block", {"minecraft:block_state": {"mode": mode}})
                      for mode in ("start", "log", "fail", "accept")]
        elif "LightBlock.setLightOnStack" in line:
            items += [stack("light", {"minecraft:block_state": {"level": str(level)}})
                      for level in range(15, -1, -1)]
        elif "generatePresetPaintings" in line or "PaintingVariantTags.PLACEABLE" in line:
            if "!variant.is" in line:
                items += paintings(False)
            elif "variant.is" in line:
                items += paintings(True)
        elif "generateInstrumentTypes" in line or "InstrumentTags.GOAT_HORNS" in line:
            if "InstrumentTags.GOAT_HORNS" in line:
                items += [stack("goat_horn", {"minecraft:instrument": f"minecraft:{i}"})
                          for i in tag("instrument", "goat_horns")]
        # Multi-line potion calls: `combat, potions, Items.TIPPED_ARROW, ...`.
        if a := re.match(r"\w+, potions, Items\.(\w+),", line):
            item = plain(a.group(1))
            items += [stack(item, {"minecraft:potion_contents": {"potion": f"minecraft:{p}"}}) for p in potions()]
    if tab["id"] == "op_blocks":
        tab["op_only"] = True
    TABS.append(tab)

# Every accept line must have been consumed.
consumed = sum(len(t["items"]) for t in TABS)
missing = sorted({e["id"][10:] for t in TABS for e in t["items"] + t["search_only"]} - pack_items)
if missing:
    raise SystemExit(f"ids the pack lacks: {missing}")
# `PotionContents.getColorOptional` over each potion's effects, and the
# name its item's translation key ends in.
effects_java = java("world/effect/MobEffects.java")
EFFECT_COLORS = {}
EFFECTS = {}
for const, body in re.findall(r"public static final Holder<MobEffect> (\w+) = register\((.*?)\);\n", effects_java, re.S):
    m = re.search(r'"(\w+)",\s*new \w+\(\s*MobEffectCategory\.(\w+),\s*(-?\d+|0x[0-9A-Fa-f]+)', body)
    if not m:
        raise SystemExit(f"no colour for effect {const}")
    EFFECT_COLORS[const] = int(m.group(3), 0) & 0xFFFFFF
    EFFECTS[const] = (f"minecraft:{m.group(1)}", m.group(2).lower())
POTIONS = {}
potions_java = java("world/item/alchemy/Potions.java")
potion_ids = dict(re.findall(r'(\w+) = create\("([^"]+)"\)', java("world/item/alchemy/PotionIds.java")))
for key, body in re.findall(r"register\(\s*PotionIds\.(\w+),\s*(new Potion\(.*?)\);\n", potions_java, re.S):
    name = re.match(r'new Potion\("(\w+)"', body).group(1)
    red = green = blue = total = 0
    listed = []
    for effect, args in re.findall(r"new MobEffectInstance\(MobEffects\.(\w+),([^)]*)\)", body):
        parts = [a.strip() for a in args.split(",") if a.strip()]
        amplifier = int(parts[1]) + 1 if len(parts) > 1 else 1
        listed.append([EFFECTS[effect][0], int(parts[0]), amplifier - 1])
        color = EFFECT_COLORS[effect]
        red += amplifier * (color >> 16 & 255)
        green += amplifier * (color >> 8 & 255)
        blue += amplifier * (color & 255)
        total += amplifier
    color = None if total == 0 else (red // total) << 16 | (green // total) << 8 | blue // total
    POTIONS[f"minecraft:{potion_ids.get(key, key.lower())}"] = {"name": name, "color": color, "effects": listed}
assert len(POTIONS) == len(potions()), (len(POTIONS), len(potions()))
RARITY = {}
for name, value in re.findall(r"public static final Item (\w+) = (.*?);\n", items_java, re.S):
    if m := re.search(r"\.rarity\(Rarity\.(\w+)\)", value):
        entry = ITEMS.get(name)
        if isinstance(entry, str):
            RARITY[f"minecraft:{entry}"] = m.group(1).lower()
CURSES = set(tag("enchantment", "curse"))
ENCHANTMENTS = {}
for name in registry("enchantment"):
    max_level = json.load(open(os.path.join(data, "enchantment", f"{name}.json")))["max_level"]
    ENCHANTMENTS[f"minecraft:{name}"] = {"max": max_level, "curse": name in CURSES}
PAINTINGS = {}
for name in registry("painting_variant"):
    v = json.load(open(os.path.join(data, "painting_variant", f"{name}.json")))
    PAINTINGS[f"minecraft:{name}"] = {k: v[k] for k in ("width", "height", "title", "author") if k in v}
EFFECT_TABLE = {i: {"category": c} for i, c in EFFECTS.values()}


def compact(value):
    return json.dumps(value, separators=(",", ":"))


lines = ['{"tabs":[']
for i, t in enumerate(TABS):
    head = {k: v for k, v in t.items() if k not in ("items", "search_only")}
    lines.append(" " + compact(head)[:-1] + ',"items":[')
    for key in ("items", "search_only"):
        if key == "search_only":
            lines.append('  ],"search_only":[')
        for j, e in enumerate(t[key]):
            lines.append("  " + compact(e) + ("," if j + 1 < len(t[key]) else ""))
    lines.append("  ]}" + ("," if i + 1 < len(TABS) else ""))
lines.append('],"potions":{')
for i, (k, v) in enumerate(POTIONS.items()):
    lines.append(f" {compact(k)}:{compact(v)}" + ("," if i + 1 < len(POTIONS) else ""))
for table, values in (("effects", EFFECT_TABLE), ("enchantments", ENCHANTMENTS), ("paintings", PAINTINGS)):
    lines.append(f'}},"{table}":{{')
    for i, (k, v) in enumerate(values.items()):
        lines.append(f" {compact(k)}:{compact(v)}" + ("," if i + 1 < len(values) else ""))
lines.append('},"rarity":{')
for i, (k, v) in enumerate(RARITY.items()):
    lines.append(f" {compact(k)}:{compact(v)}" + ("," if i + 1 < len(RARITY) else ""))
lines.append("}}")
open(out, "w").write("\n".join(lines) + "\n")
json.load(open(out))
print(len(EFFECT_COLORS), "effects", len(POTIONS), "potions", len(RARITY), "rarities")
for t in TABS:
    print(t["row"], t["column"], t["id"], t["icon"], len(t["items"]), len(t["search_only"]))
print("total", consumed)
