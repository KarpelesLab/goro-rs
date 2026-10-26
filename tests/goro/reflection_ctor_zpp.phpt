--TEST--
Reflection constructors: argument counts, 1-argument ReflectionMethod deprecation first, string/object argument checks, createFromMethodName() of internal classes
--FILE--
<?php
foreach ([fn() => new ReflectionMethod(null), fn() => new ReflectionMethod([]), fn() => new ReflectionMethod(new stdClass), fn() => new ReflectionMethod("Exception::getMessage"), fn() => ReflectionMethod::createFromMethodName("Exception::getMessage"), fn() => new ReflectionMethod("nope"), fn() => new ReflectionMethod(function() {}), fn() => new ReflectionMethod(new Exception, "getMessage"), fn() => new ReflectionFunction(null), fn() => new ReflectionObject(5), fn() => new ReflectionEnumUnitCase("x"), fn() => new ReflectionProperty("abc", [])] as $f) { try { var_dump(get_class($f())); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
--EXPECTF--

Deprecated: Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead in %s on line 2

Deprecated: ReflectionMethod::__construct(): Passing null to parameter #1 ($objectOrMethod) of type object|string is deprecated in %s on line 2
ReflectionException: ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be a valid method name

Deprecated: Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead in %s on line 2
TypeError: ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be of type object|string, array given

Deprecated: Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead in %s on line 2
ValueError: ReflectionMethod::__construct(): Argument #2 ($method) cannot be null when argument #1 ($objectOrMethod) is an object

Deprecated: Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead in %s on line 2
string(16) "ReflectionMethod"
string(16) "ReflectionMethod"

Deprecated: Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead in %s on line 2
ReflectionException: ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be a valid method name

Deprecated: Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead in %s on line 2
ValueError: ReflectionMethod::__construct(): Argument #2 ($method) cannot be null when argument #1 ($objectOrMethod) is an object
string(16) "ReflectionMethod"

Deprecated: ReflectionFunction::__construct(): Passing null to parameter #1 ($function) of type Closure|string is deprecated in %s on line 2
ReflectionException: Function () does not exist
TypeError: ReflectionObject::__construct(): Argument #1 ($object) must be of type object, int given
ArgumentCountError: ReflectionEnumUnitCase::__construct() expects exactly 2 arguments, 1 given
TypeError: ReflectionProperty::__construct(): Argument #2 ($property) must be of type string, array given
