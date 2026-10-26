--TEST--
callable return types and variadic callable parameters autoload the named classes
--FILE--
<?php
spl_autoload_register(function ($c) {
    echo "autoload $c\n";
    eval("class $c { public static function m() { return 1; } }");
});
function r(): callable { return 'H::m'; }
var_dump(r());
$f = function (callable ...$c) { echo count($c), "\n"; };
$f(['K', 'm'], 'L::m');

?>
--EXPECT--
autoload H
string(4) "H::m"
autoload K
autoload L
2
