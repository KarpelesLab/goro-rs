--TEST--
static:: and self:: calls of an undefined method fail before their arguments are evaluated
--FILE--
<?php
function f(){echo "evaluated\n"; return 1;}
abstract class Ab { abstract function am(); static function make() { return static::am2(f()); } static function make2() { return self::am3(f()); } }
class Cc extends Ab { function am() {} }
try { Cc::make(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { Cc::make2(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { Cc::make(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
Call to undefined method Cc::am2()
Call to undefined method Ab::am3()
Call to undefined method Cc::am2()
