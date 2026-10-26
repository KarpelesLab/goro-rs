--TEST--
Creating a dynamic property by reference: deprecation (name cut at NUL), readonly class forbids it
--FILE--
<?php
readonly class R {}
$r = new R; $x = 1;
try { $r->a = &$x; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $r->b = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $r->c[] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $y = &$r->d; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($r);
#[AllowDynamicProperties] class AD {}
$a = new AD; $a->x = &$x; var_dump($a);
class C {}
$c = new C; $c->q = &$x; $c->{"a\0b"} = 2; var_dump(count((array)$c));
set_error_handler(function ($n, $s) { echo "H: $s\n"; return true; });
$c = new C; $c->q = &$x; $c->q = &$x;
--EXPECTF--
Cannot create dynamic property R::$a
Cannot create dynamic property R::$b
Cannot create dynamic property R::$c
Cannot create dynamic property R::$d
object(R)#1 (0) {
}
object(AD)#2 (1) {
  ["x"]=>
  &int(1)
}

Deprecated: Creation of dynamic property C::$q is deprecated in %s on line 12

Deprecated: Creation of dynamic property C::$a is deprecated in %s on line 12
int(2)
H: Creation of dynamic property C::$q is deprecated
