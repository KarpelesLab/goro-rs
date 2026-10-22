#!/usr/bin/env python3
"""Generate crates/goro-parser/src/php_grammar.rs from php-src's grammar.

The tables are bison's LALR(1) automaton for Zend/zend_language_parser.y with
every semantic action emptied (mid-rule actions are kept as empty blocks, so
the automaton -- and therefore every "expecting ..." list in syntax error
messages -- is identical to php-src's). goro's php_recognizer.rs runs it over
the php_scanner token stream to reproduce php-src's parse errors exactly.

usage: gen_php_grammar.py /path/to/php-src [output.rs]
Requires bison >= 3.8 in $PATH.
"""
import os
import re
import subprocess
import sys
import tempfile


def strip_actions(text):
    """Empty every {...} action block of the rules section (keeping the
    braces so mid-rule actions still create their $@N rules)."""
    out = []
    i, n = 0, len(text)

    def skip_quoted(j, q):
        j += 1
        while text[j] != q:
            if text[j] == '\\':
                j += 1
            j += 1
        return j

    while i < n:
        c = text[i]
        if c in '\'"':
            j = skip_quoted(i, c)
            out.append(text[i:j + 1])
            i = j + 1
        elif text.startswith('/*', i):
            i = text.index('*/', i) + 2
        elif c == '{':
            depth, j = 0, i
            while True:
                ch = text[j]
                if ch in '\'"':
                    j = skip_quoted(j, ch)
                elif text.startswith('/*', j):
                    j = text.index('*/', j) + 1
                elif ch == '{':
                    depth += 1
                elif ch == '}':
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            out.append('{}')
            i = j + 1
        else:
            out.append(c)
            i += 1
    return ''.join(out)


def c_array(csrc, name):
    m = re.search(r'static const \w+ ' + name + r'\[\] =\s*\{(.*?)\};', csrc, re.S)
    if not m:
        raise SystemExit('array %s not found' % name)
    return [int(x) for x in re.findall(r'-?\d+', m.group(1))]


def c_define(csrc, name):
    m = re.search(r'#define ' + name + r'\s+\(?(-?\d+)\)?', csrc)
    if not m:
        raise SystemExit('define %s not found' % name)
    return int(m.group(1))


def c_strings(csrc, name):
    m = re.search(r'static const char \*const ' + name + r'\[\] =\s*\{(.*?)\};', csrc, re.S)
    body = m.group(1)
    res = []
    for item in re.findall(r'((?:"(?:[^"\\]|\\.)*"\s*)+|YY_NULLPTR)', body):
        lits = re.findall(r'"((?:[^"\\]|\\.)*)"', item)
        res.append(''.join(bytes(l, 'utf-8').decode('unicode_escape') for l in lits))
    return res


def main():
    php_src = sys.argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    out_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, '..', 'src', 'php_grammar.rs')
    src = open(os.path.join(php_src, 'Zend', 'zend_language_parser.y')).read()
    i1 = src.index('\n%%')
    i2 = src.index('\n%%', i1 + 3)
    pro, rules = src[:i1], src[i1 + 3:i2]
    rules = strip_actions(rules[rules.index('\n'):])
    decls = []
    for line in pro.split('\n'):
        l = line.strip()
        if l.startswith(('%token', '%precedence', '%left', '%right', '%nonassoc', '%expect')):
            decls.append(re.sub(r'<\w+>', '', l))
    y = '%define parse.error verbose\n' + '\n'.join(decls) + '\n%%\n' + rules + '\n%%\n'
    with tempfile.TemporaryDirectory() as td:
        td = os.environ.get('GEN_KEEP_DIR', td)
        yp = os.path.join(td, 'g.y')
        open(yp, 'w').write(y)
        subprocess.check_call(['bison', '-Wnone', '-v', '-o', os.path.join(td, 'g.c'), yp])
        csrc = open(os.path.join(td, 'g.c')).read()
        report = open(os.path.join(td, 'g.output')).read()

    # rule numbers from the report's Grammar section
    rules_by_text = {}
    lhs = None
    gram = report[report.index('\nGrammar\n'):]
    gram = gram[:gram.index('\nTerminals unused') if '\nTerminals unused' in gram[10:] else gram.index('\nTerminals, with rules')]
    for line in gram.split('\n'):
        m = re.match(r'\s*(\d+)\s+(\S+):\s*(.*)$', line)
        if m:
            lhs = m.group(2)
            rules_by_text[(lhs, m.group(3).strip())] = int(m.group(1))
            continue
        m = re.match(r'\s*(\d+)\s+\|\s*(.*)$', line)
        if m:
            rules_by_text[(lhs, m.group(2).strip())] = int(m.group(1))

    def rule(lhs, rhs):
        # The report numbers rules from 0; the C tables from 1.
        return rules_by_text[(lhs, rhs)] + 1

    special = {
        'RULE_TOP_HALT_COMPILER': rule('top_statement', '"\'__halt_compiler\'" \'(\' \')\' \';\''),
        'RULE_INNER_HALT_COMPILER': rule('inner_statement', '"\'__halt_compiler\'" \'(\' \')\' \';\''),
        'RULE_IDENTIFIER_SEMI_RESERVED': rule('identifier', 'semi_reserved'),
        'RULE_TRAIT_ALIAS_RESERVED': rule('trait_alias', 'trait_method_reference "\'as\'" reserved_non_modifiers'),
        'RULE_CLASS_CONST_SEMI_RESERVED': rule('class_const_decl', 'semi_reserved \'=\' expr backup_doc_comment'),
        'RULE_FUNCTION_NAME_READONLY': rule('function_name', '"\'readonly\'"'),
        'RULE_CALL_READONLY': rule('function_call', '"\'readonly\'" argument_list'),
        # Parser actions that check modifiers (zend_add_*_modifier(),
        # zend_modifier_list_to_flags(), zend_modifier_token_to_flag()).
        'RULE_CLASS_MODIFIERS_ADD': rule('class_modifiers', 'class_modifiers class_modifier'),
        'RULE_ANON_CLASS_MODIFIERS_ONE': rule('anonymous_class_modifiers', 'class_modifier'),
        'RULE_ANON_CLASS_MODIFIERS_ADD': rule('anonymous_class_modifiers', 'anonymous_class_modifiers class_modifier'),
        'RULE_CPP_MODIFIERS': rule('optional_cpp_modifiers', 'non_empty_member_modifiers'),
        'RULE_PROPERTY_MODIFIERS': rule('property_modifiers', 'non_empty_member_modifiers'),
        'RULE_METHOD_MODIFIERS': rule('method_modifiers', 'non_empty_member_modifiers'),
        'RULE_CONST_MODIFIERS': rule('class_const_modifiers', 'non_empty_member_modifiers'),
        'RULE_HOOK_MODIFIERS': rule('property_hook_modifiers', 'non_empty_member_modifiers'),
        'RULE_TRAIT_ALIAS_MODIFIER_NAME': rule('trait_alias', 'trait_method_reference "\'as\'" member_modifier identifier'),
        'RULE_TRAIT_ALIAS_MODIFIER': rule('trait_alias', 'trait_method_reference "\'as\'" member_modifier'),
    }

    # token kind numbers (php's T_* values) -> bison symbol numbers
    tok = re.search(r'enum yytokentype\s*\{(.*?)\};', csrc, re.S).group(1)
    token_values = re.findall(r'(\w+) = (\d+)', tok)

    arrays = {}
    for name in ['yytranslate', 'yypact', 'yydefact', 'yypgoto', 'yydefgoto', 'yytable', 'yycheck', 'yyr1', 'yyr2']:
        arrays[name] = c_array(csrc, name)
    defines = {}
    for name in ['YYFINAL', 'YYLAST', 'YYNTOKENS', 'YYPACT_NINF', 'YYTABLE_NINF', 'YYMAXUTOK']:
        defines[name] = c_define(csrc, name)
    tname = c_strings(csrc, 'yytname')

    def fmt(name, ty, vals):
        lines = []
        row = []
        for v in vals:
            row.append(str(v))
            if len(row) == 16:
                lines.append('    ' + ', '.join(row) + ',')
                row = []
        if row:
            lines.append('    ' + ', '.join(row) + ',')
        return 'pub static %s: [%s; %d] = [\n%s\n];\n' % (name, ty, len(vals), '\n'.join(lines))

    o = []
    o.append('// @generated by crates/goro-parser/tools/gen_php_grammar.py from\n'
             '// php-src Zend/zend_language_parser.y -- do not edit by hand.\n'
             '//! Bison LALR(1) tables of the PHP grammar (see php_recognizer.rs).\n'
             '#![allow(clippy::all)]\n\n')
    for k, v in defines.items():
        o.append('pub const %s: i32 = %d;\n' % (k, v))
    for k, v in special.items():
        o.append('pub const %s: i32 = %d;\n' % (k, v))
    o.append('\n/// Token kind values (`T_*` constants) by name.\n')
    o.append('pub static TOKEN_VALUES: [(&str, i32); %d] = [\n' % len(token_values))
    for n, v in token_values:
        o.append('    ("%s", %s),\n' % (n, v))
    o.append('];\n\n')
    o.append(fmt('YYTRANSLATE', 'u8', arrays['yytranslate']))
    o.append(fmt('YYPACT', 'i16', arrays['yypact']))
    o.append(fmt('YYDEFACT', 'i16', arrays['yydefact']))
    o.append(fmt('YYPGOTO', 'i16', arrays['yypgoto']))
    o.append(fmt('YYDEFGOTO', 'i16', arrays['yydefgoto']))
    o.append(fmt('YYTABLE', 'i16', arrays['yytable']))
    o.append(fmt('YYCHECK', 'i16', arrays['yycheck']))
    o.append(fmt('YYR1', 'i16', arrays['yyr1']))
    o.append(fmt('YYR2', 'i8', arrays['yyr2']))
    o.append('pub static YYTNAME: [&str; %d] = [\n' % len(tname))
    for s in tname:
        o.append('    %s,\n' % ('"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'))
    o.append('];\n')
    open(out_path, 'w').write(''.join(o))


if __name__ == '__main__':
    main()
