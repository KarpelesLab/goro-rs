--TEST--
A typed class constant type error names self/parent resolved
--FILE--
<?php
namespace N;
class P {}
class A extends P {
    const ?parent Y = null;
    const self|int X = 1.5;
}
--EXPECTF--

Fatal error: Cannot use float as value for class constant N\A::X of type N\A|int in %s on line 6
