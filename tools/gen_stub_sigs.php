<?php
/*
 * Generate crates/goro-core/src/stub_sigs.rs from php-src *.stub.php files.
 *
 * The stub files are the authoritative signatures of PHP's internal functions
 * and classes (the C arginfo is generated from them by build/gen_stub.php).
 * This script parses them with ext/tokenizer (any PHP >= 8.0 works) and
 * emits static Rust tables used by Reflection, get_class_methods() & co.
 *
 * Usage: php tools/gen_stub_sigs.php /path/to/php-src > crates/goro-core/src/stub_sigs.rs
 */

$src = $argv[1] ?? '/home/magicaltux/php-src';

// Stub files to include, mapped to the extension name reported by Reflection.
$dirs = [
    'Zend' => 'Core',
    'ext/standard' => 'standard',
    'ext/spl' => 'SPL',
    'ext/date' => 'date',
    'ext/reflection' => 'Reflection',
    'ext/json' => 'json',
    'ext/ctype' => 'ctype',
    'ext/hash' => 'hash',
    'ext/pcre' => 'pcre',
    'ext/random' => 'random',
    'ext/mbstring' => 'mbstring',
    'ext/tokenizer' => 'tokenizer',
    'ext/zlib' => 'zlib',
    'ext/bz2' => 'bz2',
    'ext/gmp' => 'gmp',
    'ext/session' => 'session',
    'ext/xml' => 'xml',
    'ext/simplexml' => 'SimpleXML',
    'ext/libxml' => 'libxml',
    'ext/xmlreader' => 'xmlreader',
    'ext/openssl' => 'openssl',
    'ext/curl' => 'curl',
    'ext/sockets' => 'sockets',
    'ext/mysqli' => 'mysqli',
    'ext/pdo' => 'PDO',
    'ext/pdo_mysql' => 'pdo_mysql',
    'ext/pgsql' => 'pgsql',
    'ext/pdo_pgsql' => 'pdo_pgsql',
    'ext/uri' => 'uri',
    'ext/bcmath' => 'bcmath',
    'ext/filter' => 'filter',
    'ext/iconv' => 'iconv',
    'ext/gettext' => 'gettext',
    'ext/shmop' => 'shmop',
    'ext/sysvsem' => 'sysvsem',
    'ext/sysvshm' => 'sysvshm',
    'ext/sysvmsg' => 'sysvmsg',
    'ext/ftp' => 'ftp',
    'ext/pcntl' => 'pcntl',
    'ext/posix' => 'posix',
    'ext/calendar' => 'calendar',
    'ext/phar' => 'Phar',
    'ext/exif' => 'exif',
    'ext/zip' => 'zip',
    'ext/fileinfo' => 'fileinfo',
    'ext/sodium' => 'sodium',
    'ext/soap' => 'soap',
    'ext/intl' => 'intl',
    // Test-only extension; its symbols only exist while it is loaded.
    'ext/zend_test' => 'zend_test',
    'ext/dom' => 'dom',
];

// U_ICU_VERSION_MAJOR_NUM goro-ext-intl reports (its data: intl crate,
// CLDR 48, ICU 78).
const ICU_VERSION_MAJOR = 78;

const F_PUBLIC = 1;
const F_PROTECTED = 2;
const F_PRIVATE = 4;
const F_STATIC = 0x10;
const F_FINAL = 0x20;
const F_ABSTRACT = 0x40;
const F_READONLY = 0x80;
const F_TENTATIVE = 0x10000;
const F_DEPRECATED = 0x20000;
const F_RETURNS_REF = 0x40000;
const F_PROTECTED_SET = 0x100000;
const F_PRIVATE_SET = 0x200000;
const F_VIRTUAL = 0x400000;
const F_NOT_SERIALIZABLE = 0x800000;
const F_ATTRIBUTE = 0x1000000;

$classes = [];
$functions = [];
$constants = [];

foreach ($dirs as $dir => $ext) {
    $files = [];
    $it = new RecursiveIteratorIterator(new RecursiveDirectoryIterator("$src/$dir", FilesystemIterator::SKIP_DOTS));
    foreach ($it as $f) {
        $p = $f->getPathname();
        if (!str_ends_with($p, '.stub.php')) continue;
        if (str_contains($p, '/tests/')) continue;
        if ($dir === 'Zend' && str_contains(substr($p, strlen("$src/Zend")), '/tests')) continue;
        $files[] = $p;
    }
    sort($files);
    foreach ($files as $p) {
        // Zend's test-only stubs are not part of a normal build.
        if ($dir !== 'ext/zend_test' && str_contains($p, 'zend_test')) continue;
        parse_file($p, $ext);
    }
}

function parse_file(string $path, string $ext): void {
    global $classes, $functions, $constants;
    $toks = token_get_all(file_get_contents($path));
    // Drop whitespace but keep comments (doc comments carry tags).
    // Preprocessor conditions are kept, except the blocks a mysqlnd build
    // (PDO_USE_MYSQLND) leaves out and the ICU version checks, evaluated
    // for the ICU version ext/intl reports (INTL_ICU_VERSION).
    $t = [];
    // One entry per open #if: [whether the enclosing code is kept, whether
    // the condition held, whether the current branch is kept].
    $stack = [];
    $keep = true;
    foreach ($toks as $tok) {
        if (is_array($tok) && $tok[0] === T_COMMENT && $tok[1][0] === '#') {
            $dir = trim($tok[1]);
            if (preg_match('/^#\s*if/', $dir)) {
                $cond = true;
                if (preg_match('/^#\s*ifndef\s+PDO_USE_MYSQLND\b/', $dir)) $cond = false;
                elseif (preg_match('/^#\s*if\s+U_ICU_VERSION_MAJOR_NUM\s*>=\s*(\d+)/', $dir, $m)) $cond = ICU_VERSION_MAJOR >= (int)$m[1];
                $stack[] = [$keep, $cond];
                $keep = $keep && $cond;
            } elseif (preg_match('/^#\s*else/', $dir) && $stack) {
                [$outer, $cond] = $stack[count($stack) - 1];
                $keep = $outer && !$cond;
            } elseif (preg_match('/^#\s*endif/', $dir) && $stack) {
                [$keep] = array_pop($stack);
            }
            continue;
        }
        if (!$keep) continue;
        if (is_array($tok)) {
            if ($tok[0] === T_WHITESPACE || $tok[0] === T_OPEN_TAG) {
                $t[] = [T_WHITESPACE, ' '];
                continue;
            }
            if ($tok[0] === T_COMMENT) continue;
            $t[] = $tok;
        } else {
            $t[] = $tok;
        }
    }
    $p = new Parser($t, $ext);
    $p->parseTop();
}

class Parser {
    public int $i = 0;
    public string $ns = '';
    public ?string $doc = null;
    public array $attrs = [];

    function __construct(public array $t, public string $ext) {}

    function peek(int $o = 0) {
        $j = $this->i;
        $n = 0;
        while ($j < count($this->t)) {
            $tok = $this->t[$j];
            if (is_array($tok) && $tok[0] === T_WHITESPACE) { $j++; continue; }
            if ($n === $o) return $tok;
            $n++;
            $j++;
        }
        return null;
    }
    function next() {
        while ($this->i < count($this->t)) {
            $tok = $this->t[$this->i++];
            if (is_array($tok) && $tok[0] === T_WHITESPACE) continue;
            return $tok;
        }
        return null;
    }
    static function text($tok): string { return is_array($tok) ? $tok[1] : $tok; }
    static function is($tok, $what): bool {
        if ($tok === null) return false;
        if (is_int($what)) return is_array($tok) && $tok[0] === $what;
        return self::text($tok) === $what || (is_array($tok) && strtolower($tok[1]) === $what);
    }

    function parseTop(): void {
        while (($tok = $this->peek()) !== null) {
            if (self::is($tok, T_DOC_COMMENT)) { $this->next(); $this->doc = $tok[1]; continue; }
            if (self::is($tok, T_ATTRIBUTE)) { $this->attrs[] = $this->readAttribute(); continue; }
            if (self::is($tok, T_NAMESPACE)) {
                $this->next();
                $name = '';
                while (($x = $this->next()) !== null && !self::is($x, ';') && !self::is($x, '{')) $name .= self::text($x);
                $this->ns = trim($name);
                continue;
            }
            if (self::is($tok, T_FUNCTION)) {
                $f = $this->parseFunction(0);
                $f['ext'] = $this->ext;
                $GLOBALS['functions'][strtolower($f['name'])] ??= $f;
                $this->reset();
                continue;
            }
            if (self::is($tok, T_CONST)) {
                $this->next();
                $c = $this->parseConstBody(0);
                foreach ($c as $cc) {
                    $cc['ext'] = $this->ext;
                    $GLOBALS['constants'][$cc['name']] ??= $cc;
                }
                $this->reset();
                continue;
            }
            if (self::is($tok, T_CLASS) || self::is($tok, T_INTERFACE) || self::is($tok, T_TRAIT) || self::is($tok, T_ENUM)
                || self::is($tok, T_ABSTRACT) || self::is($tok, T_FINAL) || self::is($tok, T_READONLY)) {
                $this->parseClass();
                $this->reset();
                continue;
            }
            $this->next();
        }
    }

    function reset(): void { $this->doc = null; $this->attrs = []; }

    function readAttribute(): string {
        $this->next(); // #[
        $depth = 1;
        $s = '';
        while (($x = $this->next()) !== null) {
            $tx = self::text($x);
            if ($tx === '[' ) $depth++;
            if ($tx === ']') { $depth--; if ($depth === 0) break; }
            $s .= $tx . ' ';
        }
        return trim($s);
    }

    function qualify(string $name): string {
        $name = trim($name);
        if ($name === '') return $name;
        if ($name[0] === '\\') return substr($name, 1);
        if ($this->ns !== '') return $this->ns . '\\' . $name;
        return $name;
    }

    function docTags(?string $doc): array {
        $tags = [];
        if ($doc === null) return $tags;
        if (preg_match_all('/@([a-z-]+)([^\n*]*)/', $doc, $m, PREG_SET_ORDER)) {
            foreach ($m as $mm) $tags[] = [$mm[1], trim($mm[2])];
        }
        return $tags;
    }

    function hasDeprecated(array $attrs, array $tags): bool {
        foreach ($attrs as $a) if (preg_match('/^\\\\?Deprecated\b/', $a)) return true;
        foreach ($tags as $t) if ($t[0] === 'deprecated') return true;
        return false;
    }

    function parseClass(): void {
        $flags = 0;
        while (true) {
            $tok = $this->next();
            if (self::is($tok, T_ABSTRACT)) { $flags |= F_ABSTRACT; continue; }
            if (self::is($tok, T_FINAL)) { $flags |= F_FINAL; continue; }
            if (self::is($tok, T_READONLY)) { $flags |= F_READONLY; continue; }
            break;
        }
        $kind = strtolower(self::text($tok));
        $name = $this->qualify(self::text($this->next()));
        $doc = $this->doc; $attrs = $this->attrs; $this->reset();
        $parent = null; $ifaces = []; $backing = '';
        while (($tok = $this->peek()) !== null && !self::is($tok, '{')) {
            if (self::is($tok, ':')) { $this->next(); $backing = strtolower(self::text($this->next())); continue; }
            if (self::is($tok, T_EXTENDS)) {
                $this->next();
                $list = $this->readNameList();
                if ($kind === 'interface') $ifaces = array_merge($ifaces, $list);
                else $parent = $list[0];
                continue;
            }
            if (self::is($tok, T_IMPLEMENTS)) { $this->next(); $ifaces = array_merge($ifaces, $this->readNameList()); continue; }
            $this->next();
        }
        $this->next(); // {
        $cls = [
            'name' => $name, 'kind' => $kind, 'flags' => $flags, 'parent' => $parent,
            'interfaces' => $ifaces, 'ext' => $this->ext, 'consts' => [], 'props' => [],
            'methods' => [], 'cases' => [], 'backing' => $backing,
            'deprecated' => $this->hasDeprecated($attrs, $this->docTags($doc)),
        ];
        foreach ($this->docTags($doc) as $t) {
            if ($t[0] === 'not-serializable') $cls['flags'] |= F_NOT_SERIALIZABLE;
        }
        foreach ($attrs as $a) {
            if (preg_match('/^\\\\?Attribute\b/', $a)) $cls['flags'] |= F_ATTRIBUTE;
        }
        // members
        while (($tok = $this->peek()) !== null) {
            if (self::is($tok, '}')) { $this->next(); break; }
            if (self::is($tok, T_DOC_COMMENT)) { $this->next(); $this->doc = $tok[1]; continue; }
            if (self::is($tok, T_ATTRIBUTE)) { $this->attrs[] = $this->readAttribute(); continue; }
            if (self::is($tok, T_USE)) { while (($x = $this->next()) !== null && !self::is($x, ';')); $this->reset(); continue; }
            if (self::is($tok, T_CASE)) {
                $this->next();
                $cname = self::text($this->next());
                $val = null;
                if (self::is($this->peek(), '=')) { $this->next(); $val = $this->readExpr([';']); }
                if (self::is($this->peek(), ';')) $this->next();
                $cls['cases'][] = ['name' => $cname, 'value' => $val];
                $this->reset();
                continue;
            }
            // modifiers
            $mflags = 0;
            while (true) {
                $tok = $this->peek();
                if (self::is($tok, T_PUBLIC)) { $this->next(); if ($this->peekSetVis()) { $mflags |= 0; } else $mflags |= F_PUBLIC; continue; }
                if (self::is($tok, T_PROTECTED)) { $this->next(); if ($this->peekSetVis()) $mflags |= F_PROTECTED_SET; else $mflags |= F_PROTECTED; continue; }
                if (self::is($tok, T_PRIVATE)) { $this->next(); if ($this->peekSetVis()) $mflags |= F_PRIVATE_SET; else $mflags |= F_PRIVATE; continue; }
                if (defined('T_PUBLIC_SET') && (self::is($tok, T_PUBLIC_SET))) { $this->next(); continue; }
                if (self::is($tok, T_STATIC)) { $this->next(); $mflags |= F_STATIC; continue; }
                if (self::is($tok, T_FINAL)) { $this->next(); $mflags |= F_FINAL; continue; }
                if (self::is($tok, T_ABSTRACT)) { $this->next(); $mflags |= F_ABSTRACT; continue; }
                if (self::is($tok, T_READONLY)) { $this->next(); $mflags |= F_READONLY; continue; }
                if (self::is($tok, T_VAR)) { $this->next(); $mflags |= F_PUBLIC; continue; }
                break;
            }
            if (($mflags & (F_PUBLIC | F_PROTECTED | F_PRIVATE)) === 0) $mflags |= F_PUBLIC;
            $tok = $this->peek();
            if (self::is($tok, T_FUNCTION)) {
                $f = $this->parseFunction($mflags);
                if ($kind === 'interface') $f['flags'] |= F_ABSTRACT;
                $cls['methods'][] = $f;
                $this->reset();
                continue;
            }
            if (self::is($tok, T_CONST)) {
                $this->next();
                foreach ($this->parseConstBody($mflags) as $c) $cls['consts'][] = $c;
                $this->reset();
                continue;
            }
            // property: [type] $name [= default] (; | hooks)
            $type = $this->readType();
            $var = $this->next();
            if (!self::is($var, T_VARIABLE)) { $this->reset(); continue; }
            $default = null;
            if (self::is($this->peek(), '=')) { $this->next(); $default = $this->readExpr([';', '{']); }
            $virtual = false;
            if (self::is($this->peek(), '{')) {
                // property hooks: skip body
                $this->next(); $depth = 1;
                $hooks = '';
                while (($x = $this->next()) !== null) {
                    if (self::is($x, '{')) $depth++;
                    if (self::is($x, '}')) { $depth--; if ($depth === 0) break; }
                    $hooks .= self::text($x);
                }
                $virtual = true;
            } elseif (self::is($this->peek(), ';')) {
                $this->next();
            }
            $tags = $this->docTags($this->doc);
            // (@virtual: a property the extension implements natively)
            foreach ($tags as $t) if ($t[0] === 'virtual') $virtual = true;
            if ($virtual) $mflags |= F_VIRTUAL;
            $cls['props'][] = ['name' => substr($var[1], 1), 'type' => $type, 'default' => $default, 'flags' => $mflags];
            $this->reset();
        }
        $GLOBALS['classes'][strtolower($name)] ??= $cls;
    }

    function peekSetVis(): bool {
        if (self::is($this->peek(), '(') && self::is($this->peek(1), 'set') && self::is($this->peek(2), ')')) {
            $this->next(); $this->next(); $this->next();
            return true;
        }
        return false;
    }

    function readNameList(): array {
        $out = [];
        $cur = '';
        while (($tok = $this->peek()) !== null) {
            if (self::is($tok, '{') || self::is($tok, T_IMPLEMENTS) || self::is($tok, T_EXTENDS)) break;
            $this->next();
            if (self::is($tok, ',')) { $out[] = $this->qualify($cur); $cur = ''; continue; }
            $cur .= self::text($tok);
        }
        if (trim($cur) !== '') $out[] = $this->qualify($cur);
        return $out;
    }

    function parseConstBody(int $flags): array {
        // [type] NAME = expr [, NAME = expr];
        $out = [];
        $type = '';
        // Typed constant: two identifiers before '='.
        $a = $this->peek(); $b = $this->peek(1);
        if (!self::is($b, '=')) $type = $this->readType();
        while (true) {
            $name = self::text($this->next());
            $val = null;
            if (self::is($this->peek(), '=')) { $this->next(); $val = $this->readExpr([';', ',']); }
            if ($val === 'UNKNOWN') $val = null;
            $tags = $this->docTags($this->doc);
            $cvalue = null;
            foreach ($tags as $t) if ($t[0] === 'cvalue') $cvalue = $t[1];
            $out[] = ['name' => $name, 'type' => $type, 'value' => $val, 'flags' => $flags,
                'deprecated' => $this->hasDeprecated($this->attrs, $tags)];
            $x = $this->next();
            if (!self::is($x, ',')) break;
        }
        return $out;
    }

    function readType(): string {
        $s = '';
        while (($tok = $this->peek()) !== null) {
            if (self::is($tok, T_VARIABLE) || self::is($tok, T_ELLIPSIS) || self::is($tok, '=')
                || self::is($tok, ';') || self::is($tok, ',') || self::is($tok, '{')) break;
            if (self::is($tok, ')') && strpos($s, '(') === false) break;
            if (self::is($tok, ')') && substr_count($s, '(') <= substr_count($s, ')')) break;
            $tx = self::text($tok);
            if ($tx === '&' || (is_array($tok) && ($tok[0] === T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG || $tok[0] === T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG))) {
                if (is_array($tok) && $tok[0] === T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG) break;
                $this->next();
                $s .= '&';
                continue;
            }
            // A bare identifier followed by '=' is a constant name, not a type.
            if (is_array($tok) && preg_match('/^[A-Za-z_]\w*$/', $tok[1]) && self::is($this->peek(1), '=')) break;
            $this->next();
            $s .= $tx;
        }
        return $this->qualifyType(trim($s));
    }

    function qualifyType(string $t): string {
        if ($t === '') return '';
        return preg_replace_callback('/\\\\?[A-Za-z_][A-Za-z0-9_\\\\]*/', function ($m) {
            $n = $m[0];
            $lower = strtolower(ltrim($n, '\\'));
            $builtin = ['int','float','string','bool','array','object','callable','iterable','mixed','void','null','never','false','true','self','static','parent','resource'];
            if (in_array($lower, $builtin, true)) return $lower;
            return $this->qualify($n);
        }, $t);
    }

    function readExpr(array $stops): string {
        $depth = 0;
        $s = '';
        while ($this->i < count($this->t)) {
            $tok = $this->t[$this->i];
            if (is_array($tok) && ($tok[0] === T_WHITESPACE || $tok[0] === T_DOC_COMMENT)) { $this->i++; $s .= ' '; continue; }
            $tx = self::text($tok);
            if ($depth === 0 && (in_array($tx, $stops, true) || $tx === ')')) break;
            if ($tx === '(' || $tx === '[' || $tx === '{') $depth++;
            if ($tx === ')' || $tx === ']' || $tx === '}') $depth--;
            $this->i++;
            $s .= $tx;
        }
        return trim(preg_replace('/\s+/', ' ', $s));
    }

    function parseFunction(int $flags): array {
        $doc = $this->doc; $attrs = $this->attrs;
        $tags = $this->docTags($doc);
        $this->next(); // function
        if (self::is($this->peek(), '&') || self::is($this->peek(), T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG)) { $this->next(); $flags |= F_RETURNS_REF; }
        $name = self::text($this->next());
        if (!($flags & (F_PUBLIC|F_PROTECTED|F_PRIVATE))) $name = $this->qualify($name);
        $this->next(); // (
        $params = [];
        $preferRef = [];
        foreach ($tags as $t) if ($t[0] === 'prefer-ref') $preferRef[] = ltrim($t[1], '$');
        while (($tok = $this->peek()) !== null && !self::is($tok, ')')) {
            $pattrs = [];
            while (self::is($this->peek(), T_ATTRIBUTE)) $pattrs[] = $this->readAttribute();
            // promoted ctor params modifiers (unlikely in stubs)
            while (self::is($this->peek(), T_PUBLIC) || self::is($this->peek(), T_PROTECTED) || self::is($this->peek(), T_PRIVATE) || self::is($this->peek(), T_READONLY)) $this->next();
            $type = $this->readType();
            $byRef = 0; $variadic = false;
            if (self::is($this->peek(), '&') || self::is($this->peek(), T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG)) { $this->next(); $byRef = 1; }
            if (self::is($this->peek(), T_ELLIPSIS)) { $this->next(); $variadic = true; }
            $var = $this->next();
            $pname = substr(self::text($var), 1);
            $default = null;
            if (self::is($this->peek(), '=')) { $this->next(); $default = $this->readExpr([',']); }
            if (in_array($pname, $preferRef, true)) $byRef = 2;
            // `= UNKNOWN`: optional, but the default isn't expressible.
            $unknownDefault = false;
            if ($default === 'UNKNOWN') { $default = null; $unknownDefault = true; }
            $sensitive = false;
            foreach ($pattrs as $a) if (preg_match('/SensitiveParameter/', $a)) $sensitive = true;
            $params[] = ['name' => $pname, 'type' => $type, 'default' => $default, 'by_ref' => $byRef, 'variadic' => $variadic, 'sensitive' => $sensitive, 'unknown_default' => $unknownDefault];
            if (self::is($this->peek(), ',')) $this->next();
        }
        $this->next(); // )
        $ret = '';
        if (self::is($this->peek(), ':')) { $this->next(); $ret = $this->readType(); }
        // body or ;
        if (self::is($this->peek(), '{')) {
            $this->next(); $depth = 1;
            while (($x = $this->next()) !== null) {
                if (self::is($x, '{')) $depth++;
                if (self::is($x, '}')) { $depth--; if ($depth === 0) break; }
            }
        } elseif (self::is($this->peek(), ';')) {
            $this->next();
        }
        foreach ($tags as $t) if ($t[0] === 'tentative-return-type') $flags |= F_TENTATIVE;
        if ($this->hasDeprecated($attrs, $tags)) $flags |= F_DEPRECATED;
        $alias = null;
        foreach ($tags as $t) if ($t[0] === 'alias' || $t[0] === 'implementation-alias') $alias = $t[1];
        return ['name' => $name, 'flags' => $flags, 'params' => $params, 'ret' => $ret, 'alias' => $alias];
    }
}

// ---------------------------------------------------------------- output
function rs(?string $s): string {
    if ($s === null) return 'None';
    return 'Some(' . rstr($s) . ')';
}
function rstr(string $s): string {
    return '"' . addcslashes($s, "\\\"\n\r\t") . '"';
}
function params_rs(array $params): string {
    $out = [];
    foreach ($params as $p) {
        $f = $p['by_ref'] | ($p['variadic'] ? 4 : 0) | ($p['sensitive'] ? 8 : 0) | ($p['unknown_default'] ? 16 : 0);
        $out[] = 'StubArg { name: ' . rstr($p['name']) . ', ty: ' . rstr($p['type']) . ', default: ' . rs($p['default']) . ', flags: ' . $f . ' }';
    }
    return '&[' . implode(', ', $out) . ']';
}
function func_rs(array $f, bool $withExt): string {
    $s = 'StubFunc { name: ' . rstr($f['name']) . ', flags: 0x' . dechex($f['flags']) . ', args: ' . params_rs($f['params']) . ', ret: ' . rstr($f['ret']);
    $s .= ', ext: ' . rstr($withExt ? $f['ext'] : '') . ' }';
    return $s;
}

echo "// @generated by tools/gen_stub_sigs.php from php-src *.stub.php files. Do not edit.\n";
echo "// Regenerate with: php tools/gen_stub_sigs.php /path/to/php-src > crates/goro-core/src/stub_sigs.rs\n\n";
echo "#![allow(clippy::all)]\n";
echo "use crate::stubs::{StubArg, StubFunc, StubClass, StubConst, StubProp, StubCase};\n\n";

ksort($functions);
echo "pub static FUNCTIONS: &[StubFunc] = &[\n";
foreach ($functions as $f) echo "    " . func_rs($f, true) . ",\n";
echo "];\n\n";

ksort($constants);
echo "pub static CONSTANTS: &[StubConst] = &[\n";
foreach ($constants as $c) {
    echo "    StubConst { name: " . rstr($c['name']) . ", ty: " . rstr($c['type']) . ", value: " . rs($c['value']) . ", flags: 0x" . dechex($c['flags'] | ($c['deprecated'] ? F_DEPRECATED : 0)) . ", ext: " . rstr($c['ext']) . " },\n";
}
echo "];\n\n";

ksort($classes);
// Extensions declaring their classes themselves (with a ClassEntry, from PHP
// source) only use their stubs for arginfo: they are kept out of CLASSES,
// which describes the classes the VM implements natively.
const SELF_DECLARED_CLASS_EXTS = ['zend_test', 'bcmath', 'filter', 'pcntl', 'Phar', 'sodium', 'soap'];
function classes_rs(string $name, array $list, bool $methodExt): void {
    echo "pub static $name: &[StubClass] = &[\n";
    foreach ($list as $c) {
        $kind = ['class' => 0, 'interface' => 1, 'trait' => 2, 'enum' => 3][$c['kind']];
        echo "    StubClass {\n";
        echo "        name: " . rstr($c['name']) . ", kind: $kind, flags: 0x" . dechex($c['flags'] | ($c['deprecated'] ? F_DEPRECATED : 0)) . ", ext: " . rstr($c['ext']) . ",\n";
        echo "        parent: " . rs($c['parent']) . ", interfaces: &[" . implode(', ', array_map('rstr', $c['interfaces'])) . "], backing: " . rstr($c['backing']) . ",\n";
        echo "        consts: &[" . implode(', ', array_map(fn($k) => "StubConst { name: " . rstr($k['name']) . ", ty: " . rstr($k['type']) . ", value: " . rs($k['value']) . ", flags: 0x" . dechex($k['flags'] | ($k['deprecated'] ? F_DEPRECATED : 0)) . ", ext: \"\" }", $c['consts'])) . "],\n";
        echo "        props: &[" . implode(', ', array_map(fn($k) => "StubProp { name: " . rstr($k['name']) . ", ty: " . rstr($k['type']) . ", default: " . rs($k['default']) . ", flags: 0x" . dechex($k['flags']) . " }", $c['props'])) . "],\n";
        echo "        cases: &[" . implode(', ', array_map(fn($k) => "StubCase { name: " . rstr($k['name']) . ", value: " . rs($k['value']) . " }", $c['cases'])) . "],\n";
        echo "        methods: &[\n";
        foreach ($c['methods'] as $m) {
            $m['ext'] = $c['ext'];
            echo "            " . func_rs($m, $methodExt) . ",\n";
        }
        echo "        ],\n";
        echo "    },\n";
    }
    echo "];\n\n";
}
classes_rs('CLASSES', array_filter($classes, fn($c) => !in_array($c['ext'], SELF_DECLARED_CLASS_EXTS, true)), false);
classes_rs('SELF_DECLARED_CLASSES', array_filter($classes, fn($c) => in_array($c['ext'], SELF_DECLARED_CLASS_EXTS, true)), true);

// Functions and methods implemented by another one (`@alias` /
// `@implementation-alias`: ZEND_FALIAS, ZEND_RAW_FENTRY to the target's
// handler): "name" or "Class::method" -> target "function" or "Class::method".
$aliases = [];
foreach ($functions as $f) if ($f['alias'] !== null) $aliases[$f['name']] = $f['alias'];
foreach ($classes as $c) foreach ($c['methods'] as $m) if ($m['alias'] !== null) $aliases[$c['name'] . '::' . $m['name']] = ltrim($m['alias'], '\\');
ksort($aliases);
echo "pub static ALIASES: &[(&str, &str)] = &[\n";
foreach ($aliases as $k => $v) echo "    (" . rstr($k) . ", " . rstr($v) . "),\n";
echo "];\n";
