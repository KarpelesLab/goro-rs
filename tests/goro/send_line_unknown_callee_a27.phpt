--TEST--
Static calls: the callee is known only for a class bound at compile time (a27)
--FILE--
<?php
interface I {}
class Plain { static function s($a = 0, $b = 0) {} }
class WithI implements I { static function s($a = 0, $b = 0) {} }
class Base { static function s($a = 0, $b = 0) {} }
class Child extends Base { static function c() {
    parent::s(
        1,
        $u1
    );
    static::s(
        1,
        $u2
    );
    self::s(
        1,
        $u3
    );
} }
Plain::s(
    1,
    $u4
);
WithI::s(
    1,
    $u5
);
Child::s(
    1,
    $u6
);
Later::s(
    1,
    $u7
);
class Later { static function s($a = 0, $b = 0) {} }
Child::c();
Plain::nope(
    1,
    $u8
);
--EXPECTF--

Warning: Undefined variable $u4 in %s on line 22

Warning: Undefined variable $u5 in %s on line 25

Warning: Undefined variable $u6 in %s on line 30

Warning: Undefined variable $u7 in %s on line 33

Warning: Undefined variable $u1 in %s on line 8

Warning: Undefined variable $u2 in %s on line 12

Warning: Undefined variable $u3 in %s on line 16

Fatal error: Uncaught Error: Call to undefined method Plain::nope() in %s:38
Stack trace:
#0 {main}
  thrown in %s on line 38
