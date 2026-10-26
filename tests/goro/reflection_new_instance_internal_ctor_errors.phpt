--TEST--
ReflectionClass::newInstance*() of internal classes: get_constructor errors, native constructors, final internal classes, isCloneable()
--FILE--
<?php
foreach (['Closure', 'Generator', 'WeakReference', 'Fiber', 'ReflectionClass', 'SplFixedArray', 'PDORow', 'Directory', 'HashContext', 'CurlHandle', 'XMLParser', 'Dom\Node'] as $c) {
  if (!class_exists($c)) { echo "nope $c\n"; continue; }
  echo "== $c\n";
  try { $o = new $c; echo "new ok\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
  try { (new ReflectionClass($c))->newInstance(); echo "ni ok\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
  try { (new ReflectionClass($c))->newInstanceArgs([]); echo "nia ok\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
  try { (new ReflectionClass($c))->newInstanceWithoutConstructor(); echo "niwc ok\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
  var_dump((new ReflectionClass($c))->isInstantiable(), (new ReflectionClass($c))->isCloneable());
}
foreach (['InternalIterator', 'PDO', 'PDOStatement', 'XMLReader', 'DOMXPath', 'DOMNodeList', 'SoapFault', 'SodiumException', 'Socket', 'InflateContext'] as $c) {
  if (!class_exists($c)) { echo "nope $c\n"; continue; }
  echo $c, ": ", var_export((new ReflectionClass($c))->isCloneable(), true), "\n";
}
--EXPECT--
== Closure
Error: Instantiation of class Closure is not allowed
Error: Instantiation of class Closure is not allowed
Error: Instantiation of class Closure is not allowed
ReflectionException: Class Closure is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(false)
bool(true)
== Generator
Error: The "Generator" class is reserved for internal use and cannot be manually instantiated
Error: The "Generator" class is reserved for internal use and cannot be manually instantiated
Error: The "Generator" class is reserved for internal use and cannot be manually instantiated
ReflectionException: Class Generator is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(true)
bool(false)
== WeakReference
Error: Direct instantiation of WeakReference is not allowed, use WeakReference::create instead
Error: Direct instantiation of WeakReference is not allowed, use WeakReference::create instead
Error: Direct instantiation of WeakReference is not allowed, use WeakReference::create instead
ReflectionException: Class WeakReference is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(true)
bool(false)
== Fiber
ArgumentCountError: Fiber::__construct() expects exactly 1 argument, 0 given
ArgumentCountError: Fiber::__construct() expects exactly 1 argument, 0 given
ArgumentCountError: Fiber::__construct() expects exactly 1 argument, 0 given
ReflectionException: Class Fiber is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(true)
bool(false)
== ReflectionClass
ArgumentCountError: ReflectionClass::__construct() expects exactly 1 argument, 0 given
ArgumentCountError: ReflectionClass::__construct() expects exactly 1 argument, 0 given
ArgumentCountError: ReflectionClass::__construct() expects exactly 1 argument, 0 given
niwc ok
bool(true)
bool(false)
== SplFixedArray
new ok
ni ok
nia ok
niwc ok
bool(true)
bool(true)
== PDORow
PDOException: You may not create a PDORow manually
PDOException: You may not create a PDORow manually
PDOException: You may not create a PDORow manually
ReflectionException: Class PDORow is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(true)
bool(false)
== Directory
Error: Cannot directly construct Directory, use dir() instead
Error: Cannot directly construct Directory, use dir() instead
Error: Cannot directly construct Directory, use dir() instead
niwc ok
bool(true)
bool(false)
== HashContext
Error: Call to private HashContext::__construct() from global scope
ReflectionException: Access to non-public constructor of class HashContext
ReflectionException: Access to non-public constructor of class HashContext
ReflectionException: Class HashContext is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(false)
bool(true)
== CurlHandle
Error: Cannot directly construct CurlHandle, use curl_init() instead
Error: Cannot directly construct CurlHandle, use curl_init() instead
Error: Cannot directly construct CurlHandle, use curl_init() instead
ReflectionException: Class CurlHandle is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(true)
bool(true)
== XMLParser
Error: Cannot directly construct XMLParser, use xml_parser_create() or xml_parser_create_ns() instead
Error: Cannot directly construct XMLParser, use xml_parser_create() or xml_parser_create_ns() instead
Error: Cannot directly construct XMLParser, use xml_parser_create() or xml_parser_create_ns() instead
ReflectionException: Class XMLParser is an internal class marked as final that cannot be instantiated without invoking its constructor
bool(true)
bool(false)
== Dom\Node
Error: Call to private Dom\Node::__construct() from global scope
ReflectionException: Access to non-public constructor of class Dom\Node
ReflectionException: Access to non-public constructor of class Dom\Node
niwc ok
bool(false)
bool(true)
InternalIterator: false
PDO: false
PDOStatement: false
XMLReader: false
DOMXPath: false
DOMNodeList: false
SoapFault: false
SodiumException: false
Socket: false
InflateContext: false
