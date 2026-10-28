#!/usr/bin/env python3
"""Generates crates/goro-ext-intl/src/data/numbers.txt, the number
formatting data of ext/intl's NumberFormatter, from the CLDR 48 JSON data
(npm packages cldr-numbers-full and cldr-core, public Unicode data).

usage: gen_intl_numbers.py <cldr-numbers-full/package> <cldr-core/package> <out>

Tab-separated lines:
  S loc  decimal group percent plus minus exponential permille infinity nan
         currencyDecimal currencyGroup          (latn symbols; "" = inherited)
  F loc  key pattern     (decimal, percent, scientific, currency, currency-alpha,
                          accounting, accounting-alpha, spacing, mingroup, numsys)
  C loc  CODE symbol narrow   (only where they differ from the code)
  N loc  CODE name one other  (display names, a subset of locales)
  D CODE digits rounding cashDigits cashRounding
  T REGION CODE               (the region's current tender currency)
  A child parent              (CLDR parentLocales)
"""
import json
import os
import sys

NAME_LOCALES = {"en", "fr", "de", "es", "it", "pt", "nl", "ja", "zh", "ru",
                "pl", "uk", "sv", "ko", "ar", "tr"}


MAJOR = {"USD", "EUR", "GBP", "JPY", "CNY", "CHF", "RUB", "INR"}
REGION_CUR = {}
LIKELY = {}


def local_cur(loc):
    """The currencies of the regions a locale names or implies."""
    parts = loc.split("-")
    region = next((p for p in parts[1:] if len(p) == 2 or p.isdigit()), None)
    if region is None:
        full = LIKELY.get(loc, LIKELY.get(parts[0], ""))
        region = next((p for p in full.split("-")[1:] if len(p) == 2), None)
    return REGION_CUR.get(region, set())


def main():
    nums, core, out = sys.argv[1:4]
    lines = []
    with open(os.path.join(core, "supplemental", "currencyData.json"), encoding="utf-8") as f:
        cd0 = json.load(f)["supplemental"]["currencyData"]
    for region, hist in cd0["region"].items():
        REGION_CUR[region] = {c for e in hist for c, i in e.items() if "_to" not in i}
    with open(os.path.join(core, "supplemental", "likelySubtags.json"), encoding="utf-8") as f:
        LIKELY.update(json.load(f)["supplemental"]["likelySubtags"])
    main_dir = os.path.join(nums, "main")
    for loc in sorted(os.listdir(main_dir)):
        try:
            with open(os.path.join(main_dir, loc, "numbers.json"), encoding="utf-8") as f:
                n = json.load(f)["main"][loc]["numbers"]
        except (FileNotFoundError, KeyError):
            n = None
        if n:
            sy = n.get("symbols-numberSystem-latn", {})
            keys = ["decimal", "group", "percentSign", "plusSign", "minusSign",
                    "exponential", "perMille", "infinity", "nan",
                    "currencyDecimal", "currencyGroup"]
            if sy:
                lines.append(["S", loc] + [sy.get(k, "") for k in keys])
            df = n.get("decimalFormats-numberSystem-latn", {})
            if "standard" in df:
                lines.append(["F", loc, "decimal", df["standard"]])
            pf = n.get("percentFormats-numberSystem-latn", {})
            if "standard" in pf:
                lines.append(["F", loc, "percent", pf["standard"]])
            sf = n.get("scientificFormats-numberSystem-latn", {})
            if "standard" in sf:
                lines.append(["F", loc, "scientific", sf["standard"]])
            cf = n.get("currencyFormats-numberSystem-latn", {})
            for k, name in [("standard", "currency"),
                            ("standard-alphaNextToNumber", "currency-alpha"),
                            ("accounting", "accounting"),
                            ("accounting-alphaNextToNumber", "accounting-alpha")]:
                if k in cf:
                    lines.append(["F", loc, name, cf[k]])
            sp = cf.get("currencySpacing", {}).get("beforeCurrency", {})
            if "insertBetween" in sp:
                lines.append(["F", loc, "spacing", sp["insertBetween"]])
            if "minimumGroupingDigits" in n:
                lines.append(["F", loc, "mingroup", str(n["minimumGroupingDigits"])])
            if n.get("defaultNumberingSystem", "latn") != "latn":
                lines.append(["F", loc, "numsys", n["defaultNumberingSystem"]])
        try:
            with open(os.path.join(main_dir, loc, "currencies.json"), encoding="utf-8") as f:
                cur = json.load(f)["main"][loc]["numbers"]["currencies"]
        except (FileNotFoundError, KeyError):
            cur = {}
        for code, c in sorted(cur.items()):
            sym = c.get("symbol", code)
            nar = c.get("symbol-alt-narrow", "")
            if sym != code or (nar and nar != sym):
                lines.append(["C", loc, code, sym, nar])
            if (loc in NAME_LOCALES or code in MAJOR or code in local_cur(loc)) and "displayName" in c:
                lines.append(["N", loc, code, c["displayName"],
                              c.get("displayName-count-one", ""),
                              c.get("displayName-count-other", "")])
    with open(os.path.join(core, "supplemental", "currencyData.json"), encoding="utf-8") as f:
        cd = json.load(f)["supplemental"]["currencyData"]
    for code, fr in sorted(cd["fractions"].items()):
        lines.append(["D", code, fr.get("_digits", "2"), fr.get("_rounding", "0"),
                      fr.get("_cashDigits", ""), fr.get("_cashRounding", "")])
    for region, hist in sorted(cd["region"].items()):
        for entry in hist:
            (code, info), = entry.items()
            if "_to" not in info and info.get("_tender", "true") != "false":
                lines.append(["T", region, code])
                break
    with open(os.path.join(core, "supplemental", "parentLocales.json"), encoding="utf-8") as f:
        pl = json.load(f)["supplemental"]["parentLocales"]["parentLocale"]
    for child, parent in sorted(pl.items()):
        lines.append(["A", child, parent])
    with open(out, "w", encoding="utf-8") as f:
        f.write("# Number formatting data from CLDR 48 (tools/gen_intl_numbers.py)\n")
        for l in lines:
            assert all("\t" not in x and "\n" not in x for x in l)
            f.write("\t".join(l) + "\n")


main()
