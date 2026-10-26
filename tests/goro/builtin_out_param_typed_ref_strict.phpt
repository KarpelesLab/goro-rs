--TEST--
builtins' out-parameters bound to typed properties, strict_types
--FILE--
<?php
declare(strict_types=1);
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
class C { public string $m = ''; public int $i = 0; public array $arr = [3, 1, 2]; public int|string $u = 0; public ?bool $b = null; public array $un; public float $f = 0.0; public static int $si = 0; }
$o = new C;
t(function () use ($o) { return str_replace('a', 'b', 'aa', $o->m) . var_export($o->m, true); });
t(function () use ($o) { return str_replace('a', 'b', 'aa', $o->f) . var_export($o->f, true); });
t(function () use ($o) { return str_replace('a', 'b', 'aa', $o->u) . var_export($o->u, true); });
t(function () use ($o) { return similar_text('ab', 'ac', $o->u) . var_export($o->u, true); });
t(function () use ($o) { return similar_text('ab', 'ac', $o->i) . var_export($o->i, true); });
t(function () use ($o) { return sort($o->arr) . json_encode($o->arr); });
t(function () use ($o) { return preg_match('/(a)/', 'a', $o->un) . json_encode($o->un); });
t(function () use ($o) { return preg_match('/(a)/', 'a', $o->b) . json_encode($o->b); });
t(function () { return preg_match_all('/a/', 'aa', C::$si); });
t(function () { return str_replace('a', 'b', 'aa', C::$si) . C::$si; });
t(function () use ($o) { $a = [1]; $r = &$a[0]; $o->i = &$r; return str_replace('a', 'b', 'aa', $a[0]) . json_encode($a) . $o->i; });
t(function () use ($o) { return preg_replace_callback('/a/', function ($m) use ($o) { $o->m = 'set'; return 'x'; }, 'aa', -1, $o->m) . var_export($o->m, true); });
t(function () use ($o) { return array_push($o->arr, 9) . json_encode($o->arr); });
t(function () use ($o) { return settype($o->i, 'string') . var_export($o->i, true); });
t(function () use ($o) { return list($o->i) = [5]; });
t(function () use ($o) { return mb_convert_variables('UTF-8', 'ISO-8859-1', $o->m) . var_export($o->m, true); });
t(function () use ($o) { $o->m = 'abc'; return extract(['m' => 1]) . var_export($o->m, true); });
--EXPECT--
TypeError: Cannot assign int to reference held by property C::$m of type string
string(5) "bb2.0"
string(3) "bb2"
TypeError: Cannot assign float to reference held by property C::$u of type string|int
TypeError: Cannot assign float to reference held by property C::$i of type int
string(8) "1[1,2,3]"
Error: Cannot access uninitialized non-nullable property C::$un by reference
TypeError: Cannot assign array to reference held by property C::$b of type ?bool
TypeError: Cannot assign array to reference held by property C::$si of type int
string(3) "bb2"
string(6) "bb[2]2"
TypeError: Cannot assign int to reference held by property C::$m of type string
string(10) "4[1,2,3,9]"
TypeError: Cannot assign string to reference held by property C::$i of type int
array(1) {
  [0]=>
  int(5)
}
string(15) "ISO-8859-1'set'"
string(6) "1'abc'"
