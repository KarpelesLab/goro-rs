--TEST--
A class declaring __toString() satisfies Stringable parameter/return/property types
--FILE--
<?php

class S { function __toString(): string { return "str"; } }
function a(Stringable $s) { return get_class($s); }
var_dump(a(new S));
function b(Stringable|string $s) { return get_class($s); }
var_dump(b(new S));
function c(string|Stringable $s) { return is_object($s); }
var_dump(c(new S));
function d(?Stringable $s) { return 1; }
var_dump(d(new S));
function e(Stringable|int $s) { return 1; }
var_dump(e(new S));
function r(): Stringable|string { return new S; }
var_dump(r() instanceof S);
class P { public Stringable|string $p; }
$p = new P; $p->p = new S; var_dump($p->p instanceof S);
--EXPECT--
string(1) "S"
string(1) "S"
bool(true)
int(1)
int(1)
bool(true)
bool(true)
