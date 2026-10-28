#!/usr/bin/env python3
"""Generate crates/goro-regex/src/unicode/tables.rs from the Unicode Character Database.

Usage: tools/gen_regex_unicode.py UCD_DIR [OUTPUT]

UCD_DIR holds the Unicode 16.0.0 UCD files (see URLS below; DerivedBidiClass.txt
comes from the extracted/ directory and emoji-data.txt from emoji/). OUTPUT
defaults to crates/goro-regex/src/unicode/tables.rs relative to the repository.
Standard library only; the output is deterministic.
"""

import os
import re
import sys

UNICODE_VERSION = (16, 0, 0)
BASE = "https://www.unicode.org/Public/16.0.0/ucd/"
URLS = [
    BASE + "UnicodeData.txt",
    BASE + "Scripts.txt",
    BASE + "ScriptExtensions.txt",
    BASE + "PropertyAliases.txt",
    BASE + "PropertyValueAliases.txt",
    BASE + "CaseFolding.txt",
    BASE + "PropList.txt",
    BASE + "DerivedCoreProperties.txt",
    BASE + "auxiliary/GraphemeBreakProperty.txt",
    BASE + "emoji/emoji-data.txt",
    BASE + "extracted/DerivedBidiClass.txt",
]

MAXCP = 0x110000
BLOCK_SHIFT = 7
BLOCK = 1 << BLOCK_SHIFT

GC_NAMES = ["Cc", "Cf", "Cn", "Co", "Cs", "Ll", "Lm", "Lo", "Lt", "Lu", "Mc", "Me", "Mn",
            "Nd", "Nl", "No", "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps", "Sc", "Sk", "Sm",
            "So", "Zl", "Zp", "Zs"]
GBP_NAMES = ["Other", "CR", "LF", "Control", "Extend", "ZWJ", "Regional_Indicator", "Prepend",
             "SpacingMark", "L", "V", "T", "LV", "LVT", "Extended_Pictographic"]
BIDI_NAMES = ["AL", "AN", "B", "BN", "CS", "EN", "ES", "ET", "FSI", "L", "LRE", "LRI", "LRO",
              "NSM", "ON", "PDF", "PDI", "R", "RLE", "RLI", "RLO", "S", "WS"]
# Binary properties accepted by \p{...} (observed on PHP 8.5), by long name.
BPROP_LONG = [
    "ASCII_Hex_Digit", "Alphabetic", "Bidi_Control", "Bidi_Mirrored", "Cased",
    "Case_Ignorable", "Changes_When_Casefolded", "Changes_When_Casemapped",
    "Changes_When_Lowercased", "Changes_When_Titlecased", "Changes_When_Uppercased", "Dash",
    "Deprecated", "Default_Ignorable_Code_Point", "Diacritic", "Emoji_Modifier_Base",
    "Emoji_Component", "Emoji_Modifier", "Emoji", "Emoji_Presentation", "Extender",
    "Extended_Pictographic", "Grapheme_Base", "Grapheme_Extend", "Grapheme_Link", "Hex_Digit",
    "ID_Compat_Math_Continue", "ID_Compat_Math_Start", "ID_Continue", "Ideographic", "ID_Start",
    "IDS_Binary_Operator", "IDS_Trinary_Operator", "IDS_Unary_Operator", "Join_Control",
    "Logical_Order_Exception", "Lowercase", "Math", "Modifier_Combining_Mark",
    "Noncharacter_Code_Point", "Pattern_Syntax", "Pattern_White_Space",
    "Prepended_Concatenation_Mark", "Quotation_Mark", "Radical", "Regional_Indicator",
    "Soft_Dotted", "Sentence_Terminal", "Terminal_Punctuation", "Unified_Ideograph",
    "Uppercase", "Variation_Selector", "White_Space", "XID_Continue", "XID_Start",
]
EXPECTED_BPROP_SHORT = [
    "AHex", "Alpha", "Bidi_C", "Bidi_M", "Cased", "CI", "CWCF", "CWCM", "CWL", "CWT", "CWU",
    "Dash", "Dep", "DI", "Dia", "EBase", "EComp", "EMod", "Emoji", "EPres", "Ext", "ExtPict",
    "Gr_Base", "Gr_Ext", "Gr_Link", "Hex", "ID_Compat_Math_Continue", "ID_Compat_Math_Start",
    "IDC", "Ideo", "IDS", "IDSB", "IDST", "IDSU", "Join_C", "LOE", "Lower", "Math", "MCM",
    "NChar", "Pat_Syn", "Pat_WS", "PCM", "QMark", "Radical", "RI", "SD", "STerm", "Term",
    "UIdeo", "Upper", "VS", "WSpace", "XIDC", "XIDS",
]
# General category groups: short name -> members.
GC_GROUPS = {
    "C": ["Cc", "Cf", "Cn", "Co", "Cs"],
    "L": ["Ll", "Lm", "Lo", "Lt", "Lu"],
    "LC": ["Ll", "Lt", "Lu"],
    "M": ["Mc", "Me", "Mn"],
    "N": ["Nd", "Nl", "No"],
    "P": ["Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps"],
    "S": ["Sc", "Sk", "Sm", "So"],
    "Z": ["Zl", "Zp", "Zs"],
}
# Extra spelling of the cased-letter group documented in pcre2pattern.
GC_GROUP_EXTRA = {"L&": "LC"}


def norm(name):
    """Unicode loose matching (UAX44-LM3): ignore case, spaces, '-' and '_'."""
    return "".join(ch for ch in name.lower() if ch not in " -_")


def lines(path):
    with open(path, encoding="utf-8") as f:
        for raw in f:
            line = raw.split("#", 1)[0].strip()
            if line:
                yield [field.strip() for field in line.split(";")]


def cprange(text):
    if ".." in text:
        a, b = text.split("..")
        return int(a, 16), int(b, 16)
    v = int(text, 16)
    return v, v


def main():
    ucd = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(
        here, "..", "crates", "goro-regex", "src", "unicode", "tables.rs")
    p = lambda name: os.path.join(ucd, name)  # noqa: E731

    # --- aliases -------------------------------------------------------------------------
    pva = {}  # prop -> list of [short, long, extra...]
    for f in lines(p("PropertyValueAliases.txt")):
        pva.setdefault(f[0], []).append(f[1:])
    prop_alias = {}  # long name -> [short, long, extra...]
    for f in lines(p("PropertyAliases.txt")):
        prop_alias[f[1]] = f

    # --- general category ----------------------------------------------------------------
    gc_idx = {n: i for i, n in enumerate(GC_NAMES)}
    gc = [gc_idx["Cn"]] * MAXCP
    bidi_m = [False] * MAXCP
    first = None
    for f in lines(p("UnicodeData.txt")):
        cp = int(f[0], 16)
        name = f[1]
        if name.endswith(", First>"):
            first = cp
            continue
        lo = cp
        if name.endswith(", Last>"):
            assert first is not None
            lo, first = first, None
        for c in range(lo, cp + 1):
            gc[c] = gc_idx[f[2]]
            bidi_m[c] = f[9] == "Y"
    assert gc[0xD800] == gc_idx["Cs"] and gc[0xE000] == gc_idx["Co"] and gc[0x10FFFD] == gc_idx["Co"]

    # --- scripts -------------------------------------------------------------------------
    sc_vals = [v for v in pva["sc"] if v[0] not in ("Zzzz", "Zyyy", "Zinh", "Hrkt")]
    sc_vals.sort(key=lambda v: v[1])
    sc_all = {v[0]: v for v in pva["sc"]}
    scripts = [sc_all["Zzzz"], sc_all["Zyyy"], sc_all["Zinh"]] + sc_vals
    assert len(scripts) < 256
    sc_by_name = {}
    for i, v in enumerate(scripts):
        for n in v:
            sc_by_name[n] = i
    script = [0] * MAXCP
    for f in lines(p("Scripts.txt")):
        lo, hi = cprange(f[0])
        for c in range(lo, hi + 1):
            script[c] = sc_by_name[f[1]]

    scx_sets = [()]
    scx_map = {(): 0}
    scx = [0] * MAXCP
    for f in lines(p("ScriptExtensions.txt")):
        lo, hi = cprange(f[0])
        key = tuple(sorted(sc_by_name[n] for n in f[1].split()))
        if key not in scx_map:
            scx_map[key] = len(scx_sets)
            scx_sets.append(key)
        for c in range(lo, hi + 1):
            scx[c] = scx_map[key]

    # --- grapheme break ------------------------------------------------------------------
    gbp_idx = {n: i for i, n in enumerate(GBP_NAMES)}
    gbp = [0] * MAXCP
    for f in lines(p("GraphemeBreakProperty.txt")):
        lo, hi = cprange(f[0])
        for c in range(lo, hi + 1):
            gbp[c] = gbp_idx[f[1]]

    # --- binary properties ---------------------------------------------------------------
    bprop_short = [prop_alias[n][0] for n in BPROP_LONG]
    assert bprop_short == EXPECTED_BPROP_SHORT, bprop_short
    assert len(BPROP_LONG) <= 64
    bp_bit = {n: i for i, n in enumerate(BPROP_LONG)}
    bmask = [0] * MAXCP
    seen = set()
    for fname in ("PropList.txt", "DerivedCoreProperties.txt", "emoji-data.txt"):
        for f in lines(p(fname)):
            if len(f) != 2:
                continue  # InCB (Indic_Conjunct_Break) has a value column: not binary
            name = f[1]
            if name not in bp_bit:
                continue
            seen.add(name)
            bit = 1 << bp_bit[name]
            lo, hi = cprange(f[0])
            for c in range(lo, hi + 1):
                bmask[c] |= bit
    bit = 1 << bp_bit["Bidi_Mirrored"]
    for c in range(MAXCP):
        if bidi_m[c]:
            bmask[c] |= bit
    seen.add("Bidi_Mirrored")
    assert seen == set(BPROP_LONG), set(BPROP_LONG) - seen

    ep_bit = 1 << bp_bit["Extended_Pictographic"]
    ep_conflicts = []
    for c in range(MAXCP):
        if bmask[c] & ep_bit:
            if gbp[c] == 0:
                gbp[c] = gbp_idx["Extended_Pictographic"]
            else:
                ep_conflicts.append(c)
    if ep_conflicts:
        print("Extended_Pictographic with non-Other GCB (kept):",
              " ".join("%04X" % c for c in ep_conflicts), file=sys.stderr)

    bprop_sets = [0]
    bprop_map = {0: 0}
    for m in bmask:
        if m not in bprop_map:
            bprop_map[m] = len(bprop_sets)
            bprop_sets.append(m)

    # --- bidi class ----------------------------------------------------------------------
    bidi_idx = {}
    for v in pva["bc"]:
        for n in v:
            bidi_idx[n] = BIDI_NAMES.index(v[0])
    bidi = [None] * MAXCP
    missing = []
    path = p("extracted/DerivedBidiClass.txt")
    if not os.path.exists(path):
        path = p("DerivedBidiClass.txt")
    with open(path, encoding="utf-8") as fh:
        for raw in fh:
            m = re.match(r"#\s*@missing:\s*([0-9A-F.]+)\s*;\s*(\w+)", raw)
            if m:
                missing.append((cprange(m.group(1)), bidi_idx[m.group(2)]))
    assert missing and missing[0][0] == (0, MAXCP - 1)
    for (lo, hi), v in missing:  # later, narrower @missing lines override the global one
        for c in range(lo, hi + 1):
            bidi[c] = v
    for f in lines(path):
        lo, hi = cprange(f[0])
        for c in range(lo, hi + 1):
            bidi[c] = bidi_idx[f[1]]
    unassigned_bidi = missing[0][1]

    # --- case equivalence sets -----------------------------------------------------------
    classes = {}
    for f in lines(p("CaseFolding.txt")):
        if f[1] not in ("C", "S"):
            continue
        src, dst = int(f[0], 16), int(f[2], 16)
        classes.setdefault(dst, {dst}).add(src)
    case_sets = [()]
    caseset = [0] * MAXCP
    for dst in sorted(classes):
        members = tuple(sorted(classes[dst]))
        if len(members) < 2:
            continue
        idx = len(case_sets)
        case_sets.append(members)
        for c in members:
            assert caseset[c] == 0, "%04X in two case sets" % c
            caseset[c] = idx
    assert len(case_sets) < 65536 and len(scx_sets) < 65536 and len(bprop_sets) < 65536

    # --- records and two-stage table -----------------------------------------------------
    records = []
    rec_map = {}
    per_cp = [0] * MAXCP
    for c in range(MAXCP):
        r = (gc[c], script[c], gbp[c], bidi[c], caseset[c], scx[c], bprop_map[bmask[c]])
        i = rec_map.get(r)
        if i is None:
            i = rec_map[r] = len(records)
            records.append(r)
        per_cp[c] = i
    assert len(records) < 65536
    stage1 = []
    stage2 = []
    block_map = {}
    for b in range(MAXCP >> BLOCK_SHIFT):
        blk = tuple(per_cp[b * BLOCK:(b + 1) * BLOCK])
        i = block_map.get(blk)
        if i is None:
            i = block_map[blk] = len(block_map)
            stage2.extend(blk)
        stage1.append(i)
    nblocks = len(block_map)

    # --- name lookup tables (normalised, sorted for binary search) -----------------------
    def lookup_table(pairs):
        d = {}
        for name, v in pairs:
            k = norm(name)
            assert d.get(k, v) == v, "name clash: %s" % name
            d[k] = v
        return sorted(d.items())

    gc_long = {v[0]: v[1:] for v in pva["gc"]}
    gc_lookup = lookup_table((n, i) for i, s in enumerate(GC_NAMES) for n in [s] + gc_long[s])
    gc_group_lookup = lookup_table(
        [(n, sum(1 << gc_idx[m] for m in GC_GROUPS[g])) for g in GC_GROUPS for n in [g] + gc_long[g]]
        + [(e, sum(1 << gc_idx[m] for m in GC_GROUPS[g])) for e, g in GC_GROUP_EXTRA.items()])
    script_lookup = lookup_table((n, i) for i, v in enumerate(scripts) for n in v)
    bprop_lookup = lookup_table((n, i) for i, ln in enumerate(BPROP_LONG) for n in prop_alias[ln])
    bidi_lookup = lookup_table((n, i) for i, s in enumerate(BIDI_NAMES)
                               for v in pva["bc"] if v[0] == s for n in v)

    # --- emit ----------------------------------------------------------------------------
    o = []
    w = o.append
    w("// @generated by tools/gen_regex_unicode.py from the Unicode Character Database %d.%d.%d."
      % UNICODE_VERSION)
    w("// Do not edit by hand; rerun the generator instead. Sources:")
    for u in URLS:
        w("//   " + u)
    w("")
    w("// rustfmt skips this file: `#[rustfmt::skip]` is on its `mod` item in unicode/mod.rs.")
    w("#![allow(clippy::all)]")
    w("")
    w("/// Unicode version these tables were generated from.")
    w("pub const UNICODE_VERSION: (u8, u8, u8) = (%d, %d, %d);" % UNICODE_VERSION)
    w("")
    w("/// Properties of one code point; `RECORDS` holds every distinct combination.")
    w("#[derive(Clone, Copy, Debug, PartialEq, Eq)]")
    w("pub struct Record {")
    w("    /// General category, index into `GC_NAMES`.")
    w("    pub gc: u8,")
    w("    /// Script, index into `SCRIPTS`.")
    w("    pub script: u8,")
    w("    /// Grapheme_Cluster_Break (Extended_Pictographic folded in), index into `GBP_NAMES`.")
    w("    pub gbp: u8,")
    w("    /// Bidi_Class, index into `BIDI_NAMES`.")
    w("    pub bidi: u8,")
    w("    /// Case-equivalence set, index into `CASE_SETS` (0 = none).")
    w("    pub caseset: u16,")
    w("    /// Script_Extensions, index into `SCX_SETS` (0 = just `script`).")
    w("    pub scx: u16,")
    w("    /// Binary properties, index into `BPROP_SETS`.")
    w("    pub bprops: u16,")
    w("}")
    w("")
    w("/// Log2 of the number of code points per STAGE2 block.")
    w("pub const BLOCK_SHIFT: u32 = %d;" % BLOCK_SHIFT)
    w("")

    def strlist(xs):
        return ", ".join('"%s"' % x for x in xs)

    def numlines(nums, per=40, fmt="%d"):
        res = []
        for i in range(0, len(nums), per):
            res.append("    " + ",".join(fmt % n for n in nums[i:i + per]) + ",")
        return res

    w("/// General category short names.")
    w("pub static GC_NAMES: [&str; %d] = [%s];" % (len(GC_NAMES), strlist(GC_NAMES)))
    for i, n in enumerate(GC_NAMES):
        w("pub const GC_%s: u8 = %d;" % (n.upper(), i))
    w("")
    w("/// Grapheme_Cluster_Break values (`Extended_Pictographic` is used for GCB=Other with ExtPict).")
    w("pub static GBP_NAMES: [&str; %d] = [%s];" % (len(GBP_NAMES), strlist(GBP_NAMES)))
    for i, n in enumerate(GBP_NAMES):
        w("pub const GBP_%s: u8 = %d;" % (n.upper(), i))
    w("")
    w("/// Bidi_Class short names.")
    w("pub static BIDI_NAMES: [&str; %d] = [%s];" % (len(BIDI_NAMES), strlist(BIDI_NAMES)))
    for i, n in enumerate(BIDI_NAMES):
        w("pub const BIDI_%s: u8 = %d;" % (n.upper(), i))
    w("/// Bidi_Class of unassigned code points outside the ranges with other defaults.")
    w("pub const BIDI_UNASSIGNED_DEFAULT: u8 = %d;" % unassigned_bidi)
    w("")
    w("/// Scripts as (short, long) names: 0 Unknown, 1 Common, 2 Inherited, then by long name.")
    w("pub static SCRIPTS: [(&str, &str); %d] = [" % len(scripts))
    for v in scripts:
        w('    ("%s", "%s"),' % (v[0], v[1]))
    w("];")
    w("pub const SCRIPT_UNKNOWN: u8 = 0;")
    w("pub const SCRIPT_COMMON: u8 = 1;")
    w("pub const SCRIPT_INHERITED: u8 = 2;")
    w("/// Extra script aliases from PropertyValueAliases.txt.")
    w("pub static SCRIPT_ALIASES: &[(&str, u8)] = &[")
    for i, v in enumerate(scripts):
        for n in v[2:]:
            w('    ("%s", %d),' % (n, i))
    w("];")
    w("")
    w("/// Binary properties as (short, long) names; bit i of a `BPROP_SETS` entry is property i.")
    w("pub static BPROPS: [(&str, &str); %d] = [" % len(BPROP_LONG))
    for s, ln in zip(bprop_short, BPROP_LONG):
        w('    ("%s", "%s"),' % (s, ln))
    w("];")
    for i, s in enumerate(bprop_short):
        w("pub const BPROP_%s: u8 = %d;" % (s.upper(), i))
    w("")
    w("/// Script_Extensions sets (sorted script indices); entry 0 means {script}.")
    w("pub static SCX_SETS: [&[u8]; %d] = [" % len(scx_sets))
    for s in scx_sets:
        w("    &[%s]," % ",".join(str(x) for x in s))
    w("];")
    w("")
    w("/// Distinct binary-property bit sets; entry 0 is the empty set.")
    w("pub static BPROP_SETS: [u64; %d] = [" % len(bprop_sets))
    o.extend(numlines(bprop_sets, 8, "0x%016x"))
    w("];")
    w("")
    w("/// Case-equivalence classes (simple case folding, statuses C and S); entry 0 is empty.")
    w("pub static CASE_SETS: [&[u32]; %d] = [" % len(case_sets))
    for i in range(0, len(case_sets), 8):
        w("    " + " ".join("&[%s]," % ",".join("0x%x" % c for c in s) for s in case_sets[i:i + 8]))
    w("];")
    w("")
    w("/// Every distinct property combination: (gc, script, gbp, bidi, caseset, scx, bprops).")
    w("pub static RECORDS: [Record; %d] = [" % len(records))
    for i in range(0, len(records), 4):
        w("    " + " ".join(
            "Record{gc:%d,script:%d,gbp:%d,bidi:%d,caseset:%d,scx:%d,bprops:%d}," % r
            for r in records[i:i + 4]))
    w("];")
    w("")
    w("/// Code point >> BLOCK_SHIFT -> block number.")
    w("pub static STAGE1: [u16; %d] = [" % len(stage1))
    o.extend(numlines(stage1, 64))
    w("];")
    w("")
    w("/// Block number * 128 + (code point & 127) -> index into `RECORDS` (%d blocks)." % nblocks)
    w("pub static STAGE2: [u16; %d] = [" % len(stage2))
    o.extend(numlines(stage2, 64))
    w("];")
    w("")

    def emit_lookup(name, doc, table, ty="u8", fmt="%d"):
        w("/// %s, keyed by loosely-normalised name (lowercase, no ' ', '-', '_'), sorted." % doc)
        w("pub static %s: [(&str, %s); %d] = [" % (name, ty, len(table)))
        for k, v in table:
            w('    ("%s", %s),' % (k, fmt % v))
        w("];")
        w("")

    emit_lookup("GC_LOOKUP", "General category names -> `GC_NAMES` index", gc_lookup)
    emit_lookup("GC_GROUP_LOOKUP", "General category group names -> bit mask over `GC_NAMES`",
                gc_group_lookup, "u32", "0x%08x")
    emit_lookup("SCRIPT_LOOKUP", "Script names and aliases -> `SCRIPTS` index", script_lookup)
    emit_lookup("BPROP_LOOKUP", "Binary property names and aliases -> `BPROPS` index", bprop_lookup)
    emit_lookup("BIDI_LOOKUP", "Bidi_Class names -> `BIDI_NAMES` index", bidi_lookup)

    text = "\n".join(o).rstrip("\n") + "\n"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)

    size = (len(stage1) * 2 + len(stage2) * 2 + len(records) * 10 + len(bprop_sets) * 8
            + sum(len(s) * 4 for s in case_sets) + sum(len(s) for s in scx_sets))
    print("records=%d blocks=%d stage1=%dB stage2=%dB records=%dB bprop_sets=%d scx_sets=%d "
          "case_sets=%d approx_total=%dB"
          % (len(records), nblocks, len(stage1) * 2, len(stage2) * 2, len(records) * 10,
             len(bprop_sets), len(scx_sets), len(case_sets), size), file=sys.stderr)


if __name__ == "__main__":
    main()
