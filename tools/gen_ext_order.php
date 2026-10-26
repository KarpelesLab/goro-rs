<?php
/*
 * Generate crates/goro-core/src/ext_order.rs: the order in which each
 * extension registers its functions and classes (what
 * ReflectionExtension::getFunctions()/getClassNames() list).
 *
 * For an extension loaded in the PHP running this script, both come from
 * its ReflectionExtension (the module's function table and the class table
 * in registration order). For the others, the functions are taken from the
 * ext's *.stub.php files in declaration order (the arginfo function table
 * follows it), and so are their classes (MINIT usually registers them in
 * stub order). Functions a loaded extension's stubs declare but this build
 * lacks (an older library, another platform) are appended to its list;
 * goro only lists the ones it defines.
 *
 * Usage: run it with the reference PHP and all its extensions loaded:
 *        php tools/gen_ext_order.php /path/to/php-src > crates/goro-core/src/ext_order.rs
 *        (then cargo fmt)
 */

$src = $argv[1] ?? '/home/magicaltux/php-src';

$functions = [];
$classes = [];

foreach (get_loaded_extensions() as $e) {
    $r = new ReflectionExtension($e);
    $key = strtolower($e);
    $functions[$key] = array_keys($r->getFunctions());
    $classes[$key] = $r->getClassNames();
}

/** Class-like names (namespaced) declared in a stub file, in order. */
function stub_classes(string $file): array
{
    $out = [];
    $ns = '';
    $tokens = token_get_all(file_get_contents($file));
    $n = count($tokens);
    for ($i = 0; $i < $n; $i++) {
        $t = $tokens[$i];
        if (!is_array($t)) {
            continue;
        }
        if ($t[0] === T_NAMESPACE) {
            $ns = '';
            for ($j = $i + 1; $j < $n && $tokens[$j] !== ';' && $tokens[$j] !== '{'; $j++) {
                if (is_array($tokens[$j]) && in_array($tokens[$j][0], [T_STRING, T_NAME_QUALIFIED], true)) {
                    $ns = $tokens[$j][1];
                }
            }
            continue;
        }
        if (in_array($t[0], [T_CLASS, T_INTERFACE, T_TRAIT, T_ENUM], true)) {
            // not `Foo::class`
            $prev = $i - 1;
            while ($prev >= 0 && is_array($tokens[$prev]) && $tokens[$prev][0] === T_WHITESPACE) {
                $prev--;
            }
            if ($prev >= 0 && is_array($tokens[$prev]) && $tokens[$prev][0] === T_DOUBLE_COLON) {
                continue;
            }
            for ($j = $i + 1; $j < $n; $j++) {
                if (is_array($tokens[$j]) && $tokens[$j][0] === T_STRING) {
                    $out[] = ($ns === '' ? '' : "$ns\\") . $tokens[$j][1];
                    break;
                }
                if ($tokens[$j] === '{' || $tokens[$j] === '(') {
                    break;
                }
            }
        }
    }
    return $out;
}

/** Top-level function names declared in a stub file, in order. */
function stub_functions(string $file): array
{
    $out = [];
    $depth = 0;
    $ns = '';
    // the brace depths at which a `namespace X { ... }` block opened
    $nsBraces = [];
    $tokens = token_get_all(file_get_contents($file));
    $n = count($tokens);
    for ($i = 0; $i < $n; $i++) {
        $t = $tokens[$i];
        if (is_array($t) && $t[0] === T_NAMESPACE && $depth === 0) {
            $ns = '';
            for ($j = $i + 1; $j < $n; $j++) {
                $u = $tokens[$j];
                if (is_array($u) && in_array($u[0], [T_STRING, T_NAME_QUALIFIED], true)) {
                    $ns = $u[1];
                } elseif ($u === ';') {
                    break;
                } elseif ($u === '{') {
                    $nsBraces[] = $depth;
                    break;
                }
            }
            $i = $j;
            continue;
        }
        if ($t === '{' || (is_array($t) && in_array($t[0], [T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES], true))) {
            $depth++;
            continue;
        }
        if ($t === '}') {
            if ($depth === 0 && $nsBraces) {
                array_pop($nsBraces);
                $ns = '';
                continue;
            }
            $depth--;
            continue;
        }
        if ($depth === 0 && is_array($t) && $t[0] === T_FUNCTION) {
            for ($j = $i + 1; $j < $n; $j++) {
                if (is_array($tokens[$j]) && $tokens[$j][0] === T_STRING) {
                    $out[] = ($ns === '' ? '' : "$ns\\") . $tokens[$j][1];
                    break;
                }
                if ($tokens[$j] === '(') {
                    break;
                }
            }
        }
    }
    return $out;
}

$dirs = array_merge(glob("$src/ext/*", GLOB_ONLYDIR), ["$src/Zend"]);
sort($dirs);
foreach ($dirs as $dir) {
    $key = basename($dir) === 'Zend' ? 'core' : strtolower(basename($dir));
    if ($key === 'opcache' && isset($functions['zend opcache'])) {
        $key = 'zend opcache';
    }
    $loaded = isset($functions[$key]);
    $stubs = glob("$dir/*.stub.php");
    sort($stubs);
    $list = $functions[$key] ?? [];
    $clist = [];
    foreach ($stubs as $stub) {
        foreach (stub_functions($stub) as $f) {
            if (!in_array($f, $list, true)) {
                $list[] = $f;
            }
        }
        foreach (stub_classes($stub) as $c) {
            if (!in_array($c, $clist, true)) {
                $clist[] = $c;
            }
        }
    }
    if ($list) {
        $functions[$key] = $list;
    }
    if (!$loaded && $clist && $key !== 'core') {
        $classes[$key] = $clist;
    }
}
ksort($functions);
ksort($classes);

function emit(string $name, array $table): void
{
    echo "pub static $name: &[(&str, &[&str])] = &[\n";
    foreach ($table as $ext => $names) {
        if (!$names) {
            continue;
        }
        echo "    (\n        \"$ext\",\n        &[\n";
        foreach ($names as $n) {
            echo "            \"", str_replace('\\', '\\\\', $n), "\",\n";
        }
        echo "        ],\n    ),\n";
    }
    echo "];\n";
}

echo "//! Generated by tools/gen_ext_order.php - do not edit.\n";
echo "//!\n";
echo "//! Per extension (lowercase name): its functions and classes in\n";
echo "//! registration order (ReflectionExtension::getFunctions() /\n";
echo "//! getClassNames()).\n\n";
emit('EXT_FUNCTIONS', $functions);
echo "\n";
emit('EXT_CLASSES', $classes);
