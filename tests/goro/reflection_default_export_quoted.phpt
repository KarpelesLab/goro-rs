--TEST--
Reflection export of non-literal parameter defaults with quoted strings
--FILE--
<?php
// Reflection's rendering of non-literal defaults (strings quoted and escaped)
class K { const A = 1; }
function f(
    $a = [K::A, 'it\'s', "c\\d", 'plain', "x\0y"],
    $b = K::A . 'tail\\',
    $c = ['k' => K::A, "q'uote" => 'v', 2 => ['n' => K::A]],
    int|string $d = K::A,
    ?K $e = null,
) {}
foreach ((new ReflectionFunction('f'))->getParameters() as $p) {
    echo str_replace("\0", '\0', (string) $p), "\n";
}
--EXPECT--
Parameter #0 [ <optional> $a = [K::A, 'it\'s', 'c\\d', 'plain', 'x\0y'] ]
Parameter #1 [ <optional> $b = K::A . 'tail\\' ]
Parameter #2 [ <optional> $c = ['k' => K::A, 'q\'uote' => 'v', 2 => ['n' => K::A]] ]
Parameter #3 [ <optional> string|int $d = K::A ]
Parameter #4 [ <optional> ?K $e = NULL ]
