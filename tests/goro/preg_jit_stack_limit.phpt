--TEST--
preg: the emulated JIT stack runs out at PHP's subject lengths
--FILE--
<?php
// The JIT stack (192 KiB) runs out at the same subject lengths as in PHP.
foreach ([
    ['/"(?:[^"\\\\]|\\\\.)*"/', '"', 'x', '"', 8191],
    ['/^(?:a|b)*$/', '', 'a', '', 8191],
    ['/^(a|b)*$/', '', 'a', '', 6143],
    ['/^(?:a)*$/', '', 'a', '', 24574],
    ['/^(?:a)+$/', '', 'a', '', 24575],
    ['/^(?>a|b)*$/', '', 'a', '', 24573],
    ['/^(?:x(y)?)*$/', '', 'xy', '', 4914],
    ['/^(?:xy*)*$/', '', 'xy', '', 8191],
    ['/^(?:\w+\s?)*$/', '', 'ab ', '', 6143],
    ['/^(?:b(?:a)+)*$/', '', 'baa', '', 8191],
    ['/^(?:(?:a)+)*$/', '', 'a', '', 24572],
    ['/^(?>(a))*$/', '', 'a', '', 3509],
    ['/^(a(?1)?)$/', '', 'a', '', 4915],
    ['/^(?:x(?=y)y)*$/', '', 'xy', '', 24574],
    ['/^(?:[^,]*,)*[^,]*$/', '', 'ab,', '', 24574],
    ['/^(?:a\s*)*$/', '', 'a', '', 8191],
] as [$re, $pre, $unit, $post, $n]) {
    $ok = preg_match($re, $pre . str_repeat($unit, $n) . $post);
    $over = preg_match($re, $pre . str_repeat($unit, $n + 1) . $post);
    echo $re, " ", var_export($ok, true), " ", var_export($over, true), " ", preg_last_error_msg(), "\n";
}
var_dump(preg_match('/^(?:a|b)*+$/', str_repeat('ab', 100000)));
ini_set('pcre.jit', '0');
var_dump(preg_match('/^(?:a|b)*$/i', str_repeat('a', 8192)));
--EXPECT--
/"(?:[^"\\]|\\.)*"/ 1 false JIT stack limit exhausted
/^(?:a|b)*$/ 1 false JIT stack limit exhausted
/^(a|b)*$/ 1 false JIT stack limit exhausted
/^(?:a)*$/ 1 false JIT stack limit exhausted
/^(?:a)+$/ 1 false JIT stack limit exhausted
/^(?>a|b)*$/ 1 false JIT stack limit exhausted
/^(?:x(y)?)*$/ 1 false JIT stack limit exhausted
/^(?:xy*)*$/ 1 false JIT stack limit exhausted
/^(?:\w+\s?)*$/ 1 false JIT stack limit exhausted
/^(?:b(?:a)+)*$/ 1 false JIT stack limit exhausted
/^(?:(?:a)+)*$/ 1 false JIT stack limit exhausted
/^(?>(a))*$/ 1 false JIT stack limit exhausted
/^(a(?1)?)$/ 1 false JIT stack limit exhausted
/^(?:x(?=y)y)*$/ 1 false JIT stack limit exhausted
/^(?:[^,]*,)*[^,]*$/ 1 false JIT stack limit exhausted
/^(?:a\s*)*$/ 1 false JIT stack limit exhausted
int(1)
int(1)
