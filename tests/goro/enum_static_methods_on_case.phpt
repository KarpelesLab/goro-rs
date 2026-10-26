--TEST--
An enum's cases()/from()/tryFrom() called on a case object ($case->cases(), [$case, 'cases'])
--FILE--
<?php
enum E: string { case FOO = 'foo'; case BAR = 'bar'; }
enum U { case A; case B; }
$o = E::FOO;
$m = 'cases';
$args = [];
var_dump(count($o->cases()));
var_dump(count($o->$m()));
var_dump(count($o->$m(...$args)));
var_dump(count(U::A->cases(...$args)));
var_dump($o->from(...['bar']));
var_dump($o->tryFrom('x'));
var_dump(call_user_func_array([$o, 'cases'], []) === E::cases());
var_dump(count(call_user_func([$o, 'cases'])));
$c = [$o, 'cases'];
var_dump(count($c()));
var_dump([$o, 'from']('bar'));
var_dump(is_callable([$o, 'cases']), is_callable([U::A, 'from']));
try { $o->cases(1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { U::A->from(1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { [U::B, 'tryFrom'](1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }

?>
--EXPECT--
int(2)
int(2)
int(2)
int(2)
enum(E::BAR)
NULL
bool(true)
int(2)
int(2)
enum(E::BAR)
bool(true)
bool(false)
ArgumentCountError: E::cases() expects exactly 0 arguments, 1 given
Error: Call to undefined method U::from()
Error: Call to undefined method U::tryFrom()
