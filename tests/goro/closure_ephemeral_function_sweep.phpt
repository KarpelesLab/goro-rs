--TEST--
Functions made for bound / static-holding closures are dropped with their Closure objects (live ones keep working)
--FILE--
<?php
class M { public static $n = 0; }
$f = function ($a = 0) { static $calls = 0; $calls++; return self::class . ":$a:$calls"; };
$keep = [];
function many() {
    $f = function () { static $x = 0; return ++$x; };
    $r = 0;
    for ($i = 0; $i < 3000; $i++) {
        $g = Closure::bind($f, null, M::class);
        $r += $g();
    }
    return $r;
}
for ($i = 0; $i < 9000; $i++) {
    $b = Closure::bind($f, null, M::class);
    if ($i % 1000 == 0) { $keep[] = $b; }
    $b($i);
}
echo (Closure::bind($f, null, M::class))(many()), "\n";
foreach ($keep as $k) { echo $k(7), " "; }
echo "\n";
$c = clone $keep[0];
for ($i = 0; $i < 9000; $i++) { $x = Closure::bind($f, null, M::class); }
echo $c(1), " ", $keep[0](2), "\n";
$gen = (Closure::bind(function () { yield self::class; yield 2; }, null, M::class))();
for ($i = 0; $i < 9000; $i++) { $x = Closure::bind($f, null, M::class); }
foreach ($gen as $v) echo $v, " ";
echo "\n";

?>
--EXPECT--
M:3000:1
M:7:2 M:7:2 M:7:2 M:7:2 M:7:2 M:7:2 M:7:2 M:7:2 M:7:2 
M:1:3 M:2:3
M 2 
