--TEST--
Native static methods/constructors name themselves in by-reference literal errors; dynamic parent callables
--FILE--
<?php
try { $u = Uri\WhatWg\Url::parse("http://x y", null, []); var_dump($u); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $u = new Uri\WhatWg\Url("http://x", null, []); var_dump($u); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$errs = 5;
$u = Uri\WhatWg\Url::parse(" http://ex.com", null, $errs);
var_dump($errs);
class A { function m() { return "A::m"; } static function s() { return "A::s " . static::class; } }
class B extends A {
  function m() { return "B::m"; }
  static function s() { return "B::s"; }
  function t() {
    var_dump(call_user_func(['parent', 'm']));
    var_dump(call_user_func('parent::m'));
    $f = ['parent', 'm']; var_dump($f());
    $f = ['parent', 's']; var_dump($f());
    $f = 'parent::s'; var_dump($f());
    $f = ['self', 'm']; var_dump($f());
    $f = ['static', 's']; var_dump($f());
    var_dump(is_callable(['parent', 'm']));
  }
}
(new B)->t();
--EXPECTF--
Error: Uri\WhatWg\Url::parse(): Argument #3 ($errors) could not be passed by reference
Error: Uri\WhatWg\Url::__construct(): Argument #3 ($softErrors) could not be passed by reference
array(1) {
  [0]=>
  object(Uri\WhatWg\UrlValidationError)#2 (3) {
    ["context"]=>
    string(14) " http://ex.com"
    ["type"]=>
    enum(Uri\WhatWg\UrlValidationErrorType::InvalidUrlUnit)
    ["failure"]=>
    bool(false)
  }
}

Deprecated: Use of "parent" in callables is deprecated in %s on line 12
string(4) "A::m"

Deprecated: Use of "parent" in callables is deprecated in %s on line 13
string(4) "A::m"

Fatal error: Uncaught Error: Class "parent" not found in %s:14
Stack trace:
#0 %s(22): B->t()
#1 {main}
  thrown in %s on line 14
