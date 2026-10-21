#!/usr/bin/env python3
"""Generate Rust HTML entity and charset tables from PHP source.

For encoding tables (codepoint → entity name) we parse the actual PHP-shipped
html_tables.h rather than regenerating from the .txt files. PHP hand-edits
its generated source to choose specific "preferred" entity names when multiple
candidates exist (e.g. `DoubleDot` over `Dot`/`die`/`uml` for U+00A8), and
those hand-edits matter for output identity.

For decoding tables (entity name → codepoint) the .txt files are authoritative
since every entry decodes to the same codepoint regardless of preference."""

import os
import re
import sys

PHP_SRC = "/home/magicaltux/php-src/ext/standard"
PHP_TABLES = f"{PHP_SRC}/html_tables"
PHP_HTML_TABLES_H = f"{PHP_SRC}/html_tables.h"
OUT_PATH = "/home/magicaltux/projects/goro-rs/crates/goro-ext-standard/src/html_tables.rs"


def parse_multi_cp_tables(text):
    """Parse multi_cp_<which>_<cp>[] tables.

    Returns dict `{(which, cp): (default_name_or_None, [(second_cp, name), ...])}`.
    First row is `leading_entry`: { default_entity, size, default_entity_len }.
    Subsequent rows are `normal_entry`: { entity, second_cp, entity_len }.
    """
    multi_pat = re.compile(
        r"static const entity_multicodepoint_row multi_cp_(?P<which>\w+)_"
        r"(?P<cp>[0-9A-Fa-f]+)\[\] = \{\s*\n"
        r"(?P<body>.*?)\n\};",
        re.DOTALL,
    )
    leading_pat = re.compile(
        r'\{\s*\{\s*(\"[^\"]*\"|NULL)\s*,\s*0?[xX]?[0-9a-fA-F]+\s*,\s*\d+\s*\}\s*\}'
    )
    normal_pat = re.compile(
        r'\{\s*\{\s*\"([^\"]+)\"\s*,\s*(0x[0-9a-fA-F]+|\d+)\s*,\s*\d+\s*\}\s*\}'
    )

    result = {}
    for m in multi_pat.finditer(text):
        which = m.group("which")
        cp = int(m.group("cp"), 16)
        body = m.group("body")
        rows = re.findall(r"\{\s*\{[^}]+\}\s*\}", body)
        if not rows:
            continue
        # First row is leading
        head = rows[0]
        hm = re.match(
            r'\{\s*\{\s*(\"[^\"]*\"|NULL)\s*,\s*(\d+)\s*,\s*\d+\s*\}\s*\}',
            head,
        )
        default = None
        if hm and hm.group(1) != "NULL":
            default = hm.group(1).strip('"')
        normals = []
        for row in rows[1:]:
            nm = re.match(
                r'\{\s*\{\s*\"([^\"]+)\"\s*,\s*(0x[0-9a-fA-F]+|\d+)\s*,\s*\d+\s*\}\s*\}',
                row,
            )
            if nm:
                name = nm.group(1)
                second_cp = int(nm.group(2), 0)
                normals.append((second_cp, name))
        result[(which, cp)] = (default, normals)
    return result


def parse_encoding_tables_from_h(path):
    """Parse PHP's html_tables.h to extract stage3 encoding tables.

    Each stage3 table is named `stage3_table_<which>_<hex_base>[]` and has 64
    entries indexed by the low 6 bits of the codepoint. Entries are either:
      - Direct: {0, { {"name", len} } } — single-cp entity
      - Multi-cp pointer: {1, { {(void *)multi_cp_<which>_<cp>, 0} } } —
        the default for this codepoint lives in multi_cp_<which>_<cp>[0],
        and additional codepoint pairs are in subsequent rows.

    Returns (single_tables, multi_tables):
      single_tables: dict `{which: [(codepoint, name), ...]}`
      multi_tables:  dict `{which: [(codepoint, second_cp, name), ...]}`"""
    with open(path) as f:
        text = f.read()

    multi_data = parse_multi_cp_tables(text)

    table_pat = re.compile(
        r"static const entity_stage3_row stage3_table_(?P<which>\w+)_"
        r"(?P<base>[0-9A-Fa-f]+)\[\] = \{\n"
        r"(?P<body>.*?)\n\};",
        re.DOTALL,
    )
    entry_iter_pat = re.compile(r"\{[01], \{ \{[^}]*\} \} \}")
    direct_pat = re.compile(r'\{0, \{ \{"([^"]+)", \d+\} \} \}')
    multi_pat_inline = re.compile(r"\{1, \{ \{\(void \*\)multi_cp_(\w+)_([0-9A-Fa-f]+), 0\} \} \}")

    single_tables = {}
    multi_tables = {}
    for m in table_pat.finditer(text):
        which = m.group("which")
        base = int(m.group("base"), 16)
        body = m.group("body")
        all_entries = entry_iter_pat.findall(body)
        if len(all_entries) != 64:
            continue
        for idx, ent in enumerate(all_entries):
            cp = base + idx
            dm = direct_pat.match(ent)
            if dm:
                single_tables.setdefault(which, []).append((cp, dm.group(1)))
                continue
            mm = multi_pat_inline.match(ent)
            if mm:
                target_which = mm.group(1)
                target_cp = int(mm.group(2), 16)
                default, normals = multi_data.get((target_which, target_cp), (None, []))
                if default is not None:
                    single_tables.setdefault(which, []).append((cp, default))
                for second_cp, ent_name in normals:
                    multi_tables.setdefault(which, []).append((cp, second_cp, ent_name))

    for k in single_tables:
        single_tables[k].sort()
    for k in multi_tables:
        multi_tables[k].sort()
    return single_tables, multi_tables


def parse_entities(path):
    """Parse PHP ents_*.txt file. Returns list of (name, [codepoints])."""
    result = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                # PHP's ents_html401.txt has comments after //
                pass
            # Strip C++ style //comments
            if "//" in line:
                line = line.split("//")[0].strip()
            if not line:
                continue
            parts = line.split()
            if len(parts) < 2:
                continue
            name = parts[0]
            cps = [int(c, 16) for c in parts[1:]]
            result.append((name, cps))
    return result


def parse_charset(path):
    """Parse 8859-X.TXT / CP*.TXT file. Returns dict {byte -> unicode_cp}."""
    result = {}
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            # Strip trailing comment
            if "#" in line:
                line = line.split("#")[0]
            parts = line.split()
            if len(parts) < 2:
                continue
            b = int(parts[0], 16)
            try:
                cp = int(parts[1], 16)
                result[b] = cp
            except ValueError:
                pass
    return result


def emit_charset_table(name, mapping):
    """Generate a 256-entry table of unicode codepoints (0 means undefined)."""
    lines = [f"pub(crate) static {name}: [u32; 256] = ["]
    for i in range(256):
        cp = mapping.get(i, 0)
        if cp == 0 and i != 0:
            lines.append(f"    0x0000, // 0x{i:02X} undefined")
        else:
            lines.append(f"    0x{cp:04X}, // 0x{i:02X}")
    lines.append("];\n")
    return "\n".join(lines)


def emit_entity_encode_from_h(name, entries):
    """Emit a sorted slice of (codepoint, entity_name) from PHP's table."""
    lines = [f"pub(crate) static {name}: &[(u32, &str)] = &["]
    for cp, ent in sorted(entries):
        lines.append(f'    (0x{cp:05X}, "{ent}"),')
    lines.append("];\n")
    return "\n".join(lines)


def emit_multi_cp_encode(name, entries):
    """Emit a sorted slice of (cp1, cp2, entity_name) for multi-cp encoding."""
    lines = [f"pub(crate) static {name}: &[(u32, u32, &str)] = &["]
    for cp1, cp2, ent in sorted(entries):
        lines.append(f'    (0x{cp1:05X}, 0x{cp2:05X}, "{ent}"),')
    lines.append("];\n")
    return "\n".join(lines)


def emit_entity_decode(name, entities):
    """Emit a sorted slice of (name, codepoint, codepoint2).

    Used for name -> codepoint lookup during decoding. cp2=0 for singles."""
    # Sort by name (case-sensitive)
    sorted_ents = sorted(entities, key=lambda x: x[0])

    lines = [f"pub(crate) static {name}: &[(&str, u32, u32)] = &["]
    for ent, cps in sorted_ents:
        cp1 = cps[0]
        cp2 = cps[1] if len(cps) >= 2 else 0
        lines.append(f'    ("{ent}", 0x{cp1:05X}, 0x{cp2:05X}),')
    lines.append("];\n")
    return "\n".join(lines)


def main():
    # Parse PHP entity files
    ents_basic = parse_entities(f"{PHP_TABLES}/ents_basic.txt")
    ents_basic_apos = parse_entities(f"{PHP_TABLES}/ents_basic_apos.txt")
    ents_html4 = parse_entities(f"{PHP_TABLES}/ents_html401.txt")
    ents_xhtml = parse_entities(f"{PHP_TABLES}/ents_xhtml.txt")
    ents_html5 = parse_entities(f"{PHP_TABLES}/ents_html5.txt")

    # Parse charset mappings
    iso88591 = parse_charset(f"{PHP_TABLES}/mappings/8859-1.TXT")
    iso885915 = parse_charset(f"{PHP_TABLES}/mappings/8859-15.TXT")
    iso88595 = parse_charset(f"{PHP_TABLES}/mappings/8859-5.TXT")
    cp1251 = parse_charset(f"{PHP_TABLES}/mappings/CP1251.TXT")
    cp1252 = parse_charset(f"{PHP_TABLES}/mappings/CP1252.TXT")
    cp866 = parse_charset(f"{PHP_TABLES}/mappings/CP866.TXT")
    koi8r = parse_charset(f"{PHP_TABLES}/mappings/KOI8-R.TXT")
    macroman = parse_charset(f"{PHP_TABLES}/mappings/ROMAN.TXT")

    with open(OUT_PATH, "w") as f:
        f.write("// AUTO-GENERATED by tools/gen_html_tables.py from php-src/ext/standard/html_tables/\n")
        f.write("// DO NOT EDIT MANUALLY. Regenerate by running the script.\n\n")
        f.write("#![allow(clippy::manual_range_contains)]\n\n")

        # Emit charset tables
        f.write(emit_charset_table("CHARSET_ISO_8859_1", iso88591))
        f.write("\n")
        f.write(emit_charset_table("CHARSET_ISO_8859_15", iso885915))
        f.write("\n")
        f.write(emit_charset_table("CHARSET_ISO_8859_5", iso88595))
        f.write("\n")
        f.write(emit_charset_table("CHARSET_CP1251", cp1251))
        f.write("\n")
        f.write(emit_charset_table("CHARSET_CP1252", cp1252))
        f.write("\n")
        f.write(emit_charset_table("CHARSET_CP866", cp866))
        f.write("\n")
        f.write(emit_charset_table("CHARSET_KOI8R", koi8r))
        f.write("\n")
        f.write(emit_charset_table("CHARSET_MACROMAN", macroman))
        f.write("\n")

        # Encoding tables: extracted from PHP's hand-curated html_tables.h to
        # match PHP's preferred entity-name choices exactly.
        enc_tables, multi_tables = parse_encoding_tables_from_h(PHP_HTML_TABLES_H)
        # PHP picks: html5 -> html5, html4/xhtml -> html4, xml1 -> basic apos
        f.write(emit_entity_encode_from_h("ENT_HTML4_ENCODE", enc_tables["html4"]))
        f.write("\n")
        f.write(emit_entity_encode_from_h("ENT_HTML5_ENCODE", enc_tables["html5"]))
        f.write("\n")
        # Multi-codepoint encoding tables: (cp1, cp2) -> name. Only HTML5 has
        # multi-cp entities (and only when all_entities=true).
        f.write(emit_multi_cp_encode("ENT_HTML5_MULTI_CP", multi_tables.get("html5", [])))
        f.write("\n")
        f.write(emit_multi_cp_encode("ENT_HTML4_MULTI_CP", multi_tables.get("html4", [])))
        f.write("\n")

        # Decoding tables (name -> codepoint) from .txt files.
        html4 = ents_basic + ents_html4
        xhtml = ents_basic_apos + ents_html4 + ents_xhtml
        html5 = ents_basic_apos + ents_html5
        f.write(emit_entity_decode("DEC_HTML4", html4))
        f.write("\n")
        f.write(emit_entity_decode("DEC_XHTML", xhtml))
        f.write("\n")
        f.write(emit_entity_decode("DEC_HTML5", html5))
        f.write("\n")

    print(f"Wrote {OUT_PATH}")
    print(f"  HTML4 enc: {len(enc_tables['html4'])} entries")
    print(f"  HTML5 enc: {len(enc_tables['html5'])} entries")
    print(f"  HTML4 dec: {len(html4)} entries")
    print(f"  XHTML dec: {len(xhtml)} entries")
    print(f"  HTML5 dec: {len(html5)} entries")


if __name__ == "__main__":
    main()
