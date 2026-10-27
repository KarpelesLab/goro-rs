--TEST--
An instance method call does not fall back to __callStatic()
--FILE--
<?php
class E { static function __callStatic($n, $a) { echo "callStatic $n\n"; } }
E::viaStatic();
$e = new E;
try { $e->viaInstance(); } catch (Error $ex) { echo $ex->getMessage(), "\n"; }
try { (new E)->other(1, 2); } catch (Error $ex) { echo $ex->getMessage(), "\n"; }
class F extends E { function t() { $this->q(); } }
try { (new F)->t(); } catch (Error $ex) { echo $ex->getMessage(), "\n"; }
call_user_func([$e, 'cuf']);
--EXPECTF--
callStatic viaStatic
Call to undefined method E::viaInstance()
Call to undefined method E::other()
Call to undefined method F::q()

Fatal error: Uncaught TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class E does not have a method "cuf" in %s:9
Stack trace:
#0 {main}
  thrown in %s on line 9
