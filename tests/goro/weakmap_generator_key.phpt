--TEST--
WeakReference and WeakMap accept a Generator as their target / key
--FILE--
<?php
function g() { yield 1; }
$gen = g();
$w = WeakReference::create($gen);
var_dump($w->get() === $gen, WeakReference::create($gen) === $w);
var_dump($w);
$wm = new WeakMap;
$wm[$gen] = 'v';
var_dump(count($wm), $wm[$gen], isset($wm[$gen]), $wm->offsetExists($gen), $wm->offsetGet($gen));
var_dump($wm);
print_r($wm); echo "\n";
foreach ($wm as $k => $v) { var_dump($k instanceof Generator, $v); }
foreach ($wm as $k => &$v) { $v = 'w'; }
unset($v);
var_dump($wm[$gen]);
$wm[$gen] .= 'x';
var_dump($wm[$gen]);
$wm2 = clone $wm;
var_dump(count($wm2));
$wm->offsetSet($gen, 5);
$wm[$gen]++;
var_dump($wm[$gen]);
$r = &$wm[$gen]; $r = 9; unset($r);
var_dump($wm[$gen]);
$g2 = g();
try { $wm[$g2]; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $wm->offsetGet($g2); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $wm[1] = 1; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $wm["x"]; } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { WeakReference::create(1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
unset($wm[$gen]);
var_dump(count($wm), count($wm2));
foreach ($wm2 as $k => $v) var_dump($k === $gen, $v);
unset($gen);
var_dump($w->get(), count($wm2));
unset($k);
var_dump($w->get(), count($wm2));
$a = g(); $b = g();
$wm3 = new WeakMap; $wm3[$a] = 1; $wm3[$b] = 2;
unset($a);
var_dump(count($wm3));
var_dump($wm3);
$o = new stdClass;
var_dump(spl_object_id($o));
// WeakMap::getIterator() and by-ref iteration with a Generator key
$it = $wm3->getIterator();
foreach ($it as $k => $v) var_dump($k === $b, $v);
foreach ($wm3 as $k => &$v) $v = 'r';
unset($v);
var_dump($wm3[$b]);
$wb = WeakReference::create($b);
unset($b);
var_dump($wb->get() === $k, count($wm3));
unset($k);
var_dump($wb->get(), count($wm3));
// cycle: generator holding map whose value references generator
function h($m) { yield 1; }
$m = new WeakMap;
$x = h($m);
$m[$x] = $x;
$w2 = WeakReference::create($x);
unset($x, $m);
var_dump($w2->get() !== null);
var_dump(gc_collect_cycles() >= 0);
var_dump($w2->get());
--EXPECT--
bool(true)
bool(true)
object(WeakReference)#2 (1) {
  ["object"]=>
  object(Generator)#1 (1) {
    ["function"]=>
    string(1) "g"
  }
}
int(1)
string(1) "v"
bool(true)
bool(true)
string(1) "v"
object(WeakMap)#3 (1) {
  [0]=>
  array(2) {
    ["key"]=>
    object(Generator)#1 (1) {
      ["function"]=>
      string(1) "g"
    }
    ["value"]=>
    string(1) "v"
  }
}
WeakMap Object
(
    [0] => Array
        (
            [key] => Generator Object
                (
                    [function] => g
                )

            [value] => v
        )

)

bool(true)
string(1) "v"
string(1) "w"
string(2) "wx"
int(1)
int(6)
int(9)
Error: Object Generator#5 not contained in WeakMap
Error: Object Generator#5 not contained in WeakMap
TypeError: WeakMap key must be an object
TypeError: WeakMap key must be an object
TypeError: WeakReference::create(): Argument #1 ($object) must be of type object, int given
int(0)
int(1)
bool(true)
string(2) "wx"
object(Generator)#1 (1) {
  ["function"]=>
  string(1) "g"
}
int(1)
NULL
int(0)
int(1)
object(WeakMap)#8 (1) {
  [0]=>
  array(2) {
    ["key"]=>
    object(Generator)#7 (1) {
      ["function"]=>
      string(1) "g"
    }
    ["value"]=>
    int(2)
  }
}
int(1)
bool(true)
int(2)
string(1) "r"
bool(true)
int(1)
NULL
int(0)
bool(true)
bool(true)
NULL
