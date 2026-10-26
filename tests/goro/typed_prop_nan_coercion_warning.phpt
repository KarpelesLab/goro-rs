--TEST--
A NAN coerced to a typed property's string or bool warns "unexpected NAN value was coerced to"
--FILE--
<?php
class P { public string $s; public bool $b; public int|string $is; public ?string $ns; public static string $st; public float $f; }
$o = new P;
$o->s = NAN; var_dump($o->s);
$o->b = NAN; var_dump($o->b);
$o->is = NAN; var_dump($o->is);
$o->ns = NAN; var_dump($o->ns);
$o->f = NAN; var_dump($o->f);
P::$st = NAN; var_dump(P::$st);
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line %d
string(3) "NAN"

Warning: unexpected NAN value was coerced to bool in %s on line %d
bool(true)

Warning: unexpected NAN value was coerced to string in %s on line %d
string(3) "NAN"

Warning: unexpected NAN value was coerced to string in %s on line %d
string(3) "NAN"
float(NAN)

Warning: unexpected NAN value was coerced to string in %s on line %d
string(3) "NAN"
