--TEST--
Closure::fromCallable() of ["C", "self::m"] / ["C", "A::m"] forms and of an object with a non-public __invoke()
--FILE--
<?php
class A { static function s() { return "A::s"; } }
class C extends A { }
class B { private function __invoke() { return "inv"; } }
try { $c = Closure::fromCallable(['C', 'self::s']); var_dump($c()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $c = Closure::fromCallable(['C', 'parent::s']); var_dump($c()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $c = Closure::fromCallable(['C', 'A::s']); var_dump($c()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $c = Closure::fromCallable(new B); var_dump($c()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(is_callable(new B)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(call_user_func(new B)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $b = new B; var_dump($b()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--

Warning: The magic method B::__invoke() must have public visibility in %s on line 4

Deprecated: Callables of the form ["C", "self::s"] are deprecated in %s on line 5
string(4) "A::s"

Deprecated: Callables of the form ["C", "parent::s"] are deprecated in %s on line 6
string(4) "A::s"

Deprecated: Callables of the form ["C", "A::s"] are deprecated in %s on line 7
string(4) "A::s"
string(3) "inv"
bool(true)
string(3) "inv"
string(3) "inv"
