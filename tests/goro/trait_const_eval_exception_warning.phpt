--TEST--
Trait constant compatibility: an exception evaluating it is reported as a warning first
--FILE--
<?php
class S { function __toString(): string { return "s"; } }
if (getenv('GORO_SURELY_UNSET_VAR')) {
    define('CONSTANT', 2);
}
trait TestTrait {
  public const Constant = 40 + CONSTANT;
}
class ComposingClass {
    use TestTrait;
    public const Constant = 42;
}
echo ComposingClass::Constant;
--EXPECTF--

Warning: Uncaught Error: Undefined constant "CONSTANT" in %s:7
Stack trace:
#0 %s(9): [constant expression]()
#1 {main}
  thrown in %s on line 7

Fatal error: ComposingClass and TestTrait define the same constant (Constant) in the composition of ComposingClass. However, the definition differs and is considered incompatible. Class was composed in %s on line 9
