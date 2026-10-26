--TEST--
Weak mode: internal Stringable objects (Exception, Error) coerce to string params and return types; $generator::class
--FILE--
<?php
// symfony/string LazyString::resolve(): an internal Stringable object
// (Exception) returned or passed for a string type in weak mode.
class S { function __toString(): string { return "S!"; } }
function r1(\Stringable|string|int|float|bool $v): string { return $v; }
function r3($v): string { return $v; }
function r4($v): ?string { return $v; }
var_dump(r1(new S), r3(new S), r4(new S));
var_dump(str_starts_with(r1(new Exception('hello')), 'Exception: hello'));
var_dump(str_starts_with(r4(new Error('e')), 'Error: e'));
function p(string $s) { return $s; }
var_dump(str_starts_with(p(new Exception('hi')), 'Exception: hi'));
class T { public string $s = ''; }
$t = new T;
$t->s = new Exception('prop');
var_dump(str_starts_with($t->s, 'Exception: prop'));
function g() { yield 1; }
$gen = g();
var_dump($gen::class);

?>
--EXPECT--
string(2) "S!"
string(2) "S!"
string(2) "S!"
bool(true)
bool(true)
bool(true)
bool(true)
string(9) "Generator"
