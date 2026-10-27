--TEST--
ReflectionConstant/ReflectionExtension constructor exceptions have the constructor frame
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; } }
t(fn() => new ReflectionConstant('NOPE_X'));
t(fn() => new ReflectionClass('NopeClass'));
t(fn() => new ReflectionFunction('nope_fn'));
t(fn() => new ReflectionMethod('stdClass', 'nope'));
t(fn() => new ReflectionProperty('stdClass', 'nope'));
t(fn() => new ReflectionClassConstant('stdClass', 'NOPE'));
t(fn() => new ReflectionEnum('stdClass'));
t(fn() => new ReflectionParameter('strlen', 5));
t(fn() => new ReflectionExtension('nope_ext'));
t(fn() => new DateTime('garbage-x'));
t(fn() => new SplFileObject('/nonexistent/x'));
t(fn() => new ArrayIterator(5));
--EXPECTF--
ReflectionException: Constant "NOPE_X" does not exist
#0 %s(3): ReflectionConstant->__construct('NOPE_X')
#1 %s(2): {closure:%s:3}()
#2 %s(3): t(Object(Closure))
#3 {main}
ReflectionException: Class "NopeClass" does not exist
#0 %s(4): ReflectionClass->__construct('NopeClass')
#1 %s(2): {closure:%s:4}()
#2 %s(4): t(Object(Closure))
#3 {main}
ReflectionException: Function nope_fn() does not exist
#0 %s(5): ReflectionFunction->__construct('nope_fn')
#1 %s(2): {closure:%s:5}()
#2 %s(5): t(Object(Closure))
#3 {main}
ReflectionException: Method stdClass::nope() does not exist
#0 %s(6): ReflectionMethod->__construct('stdClass', 'nope')
#1 %s(2): {closure:%s:6}()
#2 %s(6): t(Object(Closure))
#3 {main}
ReflectionException: Property stdClass::$nope does not exist
#0 %s(7): ReflectionProperty->__construct('stdClass', 'nope')
#1 %s(2): {closure:%s:7}()
#2 %s(7): t(Object(Closure))
#3 {main}
ReflectionException: Constant stdClass::NOPE does not exist
#0 %s(8): ReflectionClassConstant->__construct('stdClass', 'NOPE')
#1 %s(2): {closure:%s:8}()
#2 %s(8): t(Object(Closure))
#3 {main}
ReflectionException: Class "stdClass" is not an enum
#0 %s(9): ReflectionEnum->__construct('stdClass')
#1 %s(2): {closure:%s:9}()
#2 %s(9): t(Object(Closure))
#3 {main}
ReflectionException: The parameter specified by its offset could not be found
#0 %s(10): ReflectionParameter->__construct('strlen', 5)
#1 %s(2): {closure:%s:10}()
#2 %s(10): t(Object(Closure))
#3 {main}
ReflectionException: Extension "nope_ext" does not exist
#0 %s(11): ReflectionExtension->__construct('nope_ext')
#1 %s(2): {closure:%s:11}()
#2 %s(11): t(Object(Closure))
#3 {main}
DateMalformedStringException: Failed to parse time string (garbage-x) at position 0 (g): The timezone could not be found in the database
#0 %s(12): DateTime->__construct('garbage-x')
#1 %s(2): {closure:%s:12}()
#2 %s(12): t(Object(Closure))
#3 {main}
RuntimeException: SplFileObject::__construct(/nonexistent/x): Failed to open stream: No such file or directory
#0 %s(13): SplFileObject->__construct('/nonexistent/x')
#1 %s(2): {closure:%s:13}()
#2 %s(13): t(Object(Closure))
#3 {main}
TypeError: ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given
#0 %s(14): ArrayIterator->__construct(5)
#1 %s(2): {closure:%s:14}()
#2 %s(14): t(Object(Closure))
#3 {main}
