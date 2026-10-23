--TEST--
Compile-time diagnostics are raised in source order
--FILE--
<?php
// Compile-time diagnostics come in source order: the "continue" warning
// on line 6 is raised before the ${var} deprecation further down.
function sw($v) {
    switch ($v) {
        case 1: continue;
    }
    return $v;
}
echo "start\n";
$name = 'x';
echo "${name}\n";
--EXPECTF--

Warning: "continue" targeting switch is equivalent to "break" in %s on line 6

Deprecated: Using ${var} in strings is deprecated, use {$var} instead in %s on line 12
start
x
