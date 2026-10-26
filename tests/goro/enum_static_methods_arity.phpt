--TEST--
Enum from()/tryFrom()/cases() check their arguments; RecursiveIteratorIterator takes an object
--INI--
zend.exception_ignore_args=0
zend.exception_string_param_max_len=15
--FILE--
<?php
enum E: string { case A = "a"; }
enum P { case X; }
foreach ([fn() => E::cases(1), fn() => E::from(), fn() => E::from("a", 2), fn() => E::tryFrom(), fn() => P::cases(1, 2), fn() => RoundingMode::cases(1), fn() => PropertyHookType::from(), fn() => E::from(...)(), fn() => call_user_func('E::from'), fn() => E::tryFrom(value: 'a', x: 1), fn() => E::from(value: 'a')] as $f) {
    try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
try { new RecursiveTreeIterator("abc"); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { new RecursiveIteratorIterator("abc"); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { new RecursiveIteratorIterator(new stdClass); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
?>
--EXPECTF--
ArgumentCountError: E::cases() expects exactly 0 arguments, 1 given
ArgumentCountError: E::from() expects exactly 1 argument, 0 given
ArgumentCountError: E::from() expects exactly 1 argument, 2 given
ArgumentCountError: E::tryFrom() expects exactly 1 argument, 0 given
ArgumentCountError: P::cases() expects exactly 0 arguments, 2 given
ArgumentCountError: RoundingMode::cases() expects exactly 0 arguments, 1 given
ArgumentCountError: PropertyHookType::from() expects exactly 1 argument, 0 given
ArgumentCountError: E::from() expects exactly 1 argument, 0 given
ArgumentCountError: E::from() expects exactly 1 argument, 0 given
Error: Unknown named parameter $x
enum(E::A)
TypeError: RecursiveTreeIterator::__construct(): Argument #1 ($iterator) must be of type object, string given
TypeError: RecursiveIteratorIterator::__construct(): Argument #1 ($iterator) must be of type object, string given
InvalidArgumentException: An instance of RecursiveIterator or IteratorAggregate creating it is required
