--TEST--
Attribute flags of the wrong type throw a plain Error from newInstance()
--FILE--
<?php
#[Attribute("x")]
class X {}
#[X]
function f() {}
try { (new ReflectionFunction('f'))->getAttributes()[0]->newInstance(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
#[Attribute(1.5)]
class Y {}
#[Y]
function g() {}
try { (new ReflectionFunction('g'))->getAttributes()[0]->newInstance(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
#[Attribute(flags: "x")]
class Z {}
#[Z]
function h() {}
try { (new ReflectionFunction('h'))->getAttributes()[0]->newInstance(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
#[Attribute(Attribute::TARGET_CLASS, 5)]
class W {}
#[W]
function i() {}
try { (new ReflectionFunction('i'))->getAttributes()[0]->newInstance(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
Error: Attribute::__construct(): Argument #1 ($flags) must be of type int, string given
Error: Attribute::__construct(): Argument #1 ($flags) must be of type int, float given
Error: Attribute::__construct(): Argument #1 ($flags) must be of type int, string given
Error: Attribute "W" cannot target function (allowed targets: class)
