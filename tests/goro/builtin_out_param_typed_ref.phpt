--TEST--
builtins' by-reference out-parameters bound to typed properties are coerced or rejected
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$o = new class { public string $m = ''; public int $i = 0; public ?array $a = null; public float $f = 0.0; };
t(function () use ($o) { return preg_match('/a/', 'a', $o->m); });
t(function () use ($o) { return preg_match_all('/a/', 'a', $o->m); });
t(function () use ($o) { return preg_replace('/a/', 'b', 'a', -1, $o->m) . var_export($o->m, true); });
t(function () use ($o) { return preg_replace('/a/', 'b', 'a', -1, $o->f) . var_export($o->f, true); });
t(function () use ($o) { parse_str('a=1', $o->m); });
t(function () use ($o) { return sscanf('12', '%d', $o->m) . var_export($o->m, true); });
t(function () use ($o) { return sscanf('ab', '%s', $o->i) . var_export($o->i, true); });
t(function () use ($o) { return str_replace('a', 'b', 'a', $o->m) . var_export($o->m, true); });
t(function () use ($o) { return similar_text('a', 'b', $o->i) . var_export($o->i, true); });
t(function () use ($o) { return similar_text('a', 'b', $o->m) . var_export($o->m, true); });
t(function () use ($o) { return mb_parse_str('a=1', $o->m); });
t(function () use ($o) { return preg_match('/a/', 'a', $o->a) . json_encode($o->a); });
t(function () use ($o) { $r = &$o->i; return preg_match('/a/', 'a', $r); });
t(function () use ($o) { return is_callable('strlen', false, $o->i) . var_export($o->i, true); });
t(function () use ($o) { return json_encode($o); });
--EXPECT--
TypeError: Cannot assign array to reference held by property class@anonymous::$m of type string
TypeError: Cannot assign array to reference held by property class@anonymous::$m of type string
string(4) "b'1'"
string(4) "b1.0"
TypeError: Cannot assign array to reference held by property class@anonymous::$m of type string
string(5) "1'12'"
TypeError: Cannot assign string to reference held by property class@anonymous::$i of type int
string(4) "b'1'"
string(2) "00"
string(4) "0'0'"
TypeError: Cannot assign array to reference held by property class@anonymous::$m of type string
string(6) "1["a"]"
TypeError: Cannot assign array to reference held by property class@anonymous::$i of type int
TypeError: Cannot assign string to reference held by property class@anonymous::$i of type int
string(31) "{"m":"0","i":0,"a":["a"],"f":1}"
