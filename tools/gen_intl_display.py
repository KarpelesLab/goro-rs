#!/usr/bin/env python3
"""Generates crates/goro-ext-intl/data/display.txt.z (raw DEFLATE), the locale display
names ext/intl's Locale::getDisplay*() need, from the CLDR 48 JSON data
(npm packages cldr-localenames-full and cldr-numbers-full, public Unicode
data).

usage: gen_intl_display.py <cldr-localenames-full/package> <cldr-numbers-full/package> <out>

Output lines are tab-separated: kind, display locale, code, name. Kinds:
L language, S script, s script (stand-alone), R region, V variant,
K keyword key, T keyword type ("key/type"), C currency name, P pattern
(code: pattern, separator, keytype).
"""
import json
import zlib
import os
import sys

# Display locales carried (and their CLDR parents through the main chain).
LOCALES = [
    "en", "fr", "de", "es", "it", "pt", "nl", "ja", "zh", "ru", "ar", "pl",
    "sv", "ko", "tr", "cs", "da", "fi", "nb", "el", "he", "hu", "uk", "hi",
    "en-GB", "fr-CA", "pt-PT", "es-419", "zh-Hant",
]

KEYS_WITH_TYPES = {"calendar", "collation", "numbers", "colStrength",
                   "colAlternate", "colBackwards", "colCaseFirst",
                   "colCaseLevel", "colNormalization", "colNumeric", "hc",
                   "ms", "cf", "lb", "lw", "ss", "fw", "em", "va", "d0",
                   "h0", "i0", "k0", "kv", "m0", "mu", "s0", "t0"}


def load(path, *keys):
    try:
        with open(path, encoding="utf-8") as f:
            d = json.load(f)
    except FileNotFoundError:
        return {}
    for k in keys:
        d = d.get(k, {})
    return d


def main():
    names, nums, out = sys.argv[1:4]
    lines = []
    for loc in LOCALES:
        base = os.path.join(names, "main", loc)
        ldn = load(os.path.join(base, "localeDisplayNames.json"), "main", loc,
                   "localeDisplayNames")
        langs = load(os.path.join(base, "languages.json"), "main", loc,
                     "localeDisplayNames", "languages")
        scripts = load(os.path.join(base, "scripts.json"), "main", loc,
                       "localeDisplayNames", "scripts")
        terr = load(os.path.join(base, "territories.json"), "main", loc,
                    "localeDisplayNames", "territories")
        variants = load(os.path.join(base, "variants.json"), "main", loc,
                        "localeDisplayNames", "variants")
        cur = load(os.path.join(nums, "main", loc, "currencies.json"), "main",
                   loc, "numbers", "currencies")
        for k, v in langs.items():
            if "-alt-" in k:
                continue
            lines.append(("L", loc, k, v))
        for k, v in scripts.items():
            if k.endswith("-alt-stand-alone"):
                lines.append(("s", loc, k[: -len("-alt-stand-alone")], v))
            elif "-alt-" not in k:
                lines.append(("S", loc, k, v))
        for k, v in terr.items():
            if "-alt-" not in k:
                lines.append(("R", loc, k, v))
        for k, v in variants.items():
            if "-alt-" not in k:
                lines.append(("V", loc, k, v))
        for k, v in ldn.get("keys", {}).items():
            lines.append(("K", loc, k, v))
        for key, types in ldn.get("types", {}).items():
            if key not in KEYS_WITH_TYPES:
                continue
            for t, v in types.items():
                if "-alt-" in t:
                    continue
                lines.append(("T", loc, key + "/" + t, v))
        for code, c in cur.items():
            if "displayName" in c:
                lines.append(("C", loc, code, c["displayName"]))
        p = ldn.get("localeDisplayPattern", {})
        if p:
            lines.append(("P", loc, "pattern", p.get("localePattern", "")))
            lines.append(("P", loc, "separator", p.get("localeSeparator", "")))
            lines.append(("P", loc, "keytype", p.get("localeKeyTypePattern", "")))
    text = "# Locale display names from CLDR 48 (tools/gen_intl_display.py)\n"
    for kind, loc, code, name in lines:
        assert "\t" not in name and "\n" not in name
        text += f"{kind}\t{loc}\t{code}\t{name}\n"
    # raw DEFLATE (RFC 1951), inflated on first use
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    with open(out, "wb") as f:
        f.write(c.compress(text.encode("utf-8")) + c.flush())


main()
