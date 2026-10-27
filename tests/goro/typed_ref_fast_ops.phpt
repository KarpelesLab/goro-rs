--TEST--
Typed references bound while a function runs: writes through them are checked
--FILE--
<?php
class TX { public int $p = 0; public ?string $s = null; }
function setv(&$v, $x) { $v = $x; }
function run(TX $o) {
    $a = 1;
    $a += 2;
    $r = &$o->p;
    $r = "5";
    var_dump($o->p);
    $r += 1;
    $r++;
    var_dump($o->p);
    try {
        $r = "abc";
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
    try {
        $r .= "x";
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
    $q = &$r;
    $q = 9;
    var_dump($o->p);
    try {
        $q = [];
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
    setv($o->s, 12);
    var_dump($o->s);
    $t = &$o->s;
    for ($i = 0; $i < 3; $i++) {
        $t = $i;
    }
    var_dump($o->s);
    $f = function () use (&$a) { $a = "str"; };
    $f();
    var_dump($a);
    try {
        $r = null;
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
    return $a;
}
$keep = new TX;
$kr = &$keep->p;
run(new TX);
echo "done\n";
?>
--EXPECT--
int(5)
int(7)
Cannot assign string to reference held by property TX::$p of type int
Cannot assign string to reference held by property TX::$p of type int
int(9)
Cannot assign array to reference held by property TX::$p of type int
string(2) "12"
string(1) "2"
string(3) "str"
Cannot assign null to reference held by property TX::$p of type int
done
