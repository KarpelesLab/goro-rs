--TEST--
Internal functions and classes, their hierarchies and arginfo, found in any case
--FILE--
<?php
// Internal functions and classes found in any case (stub name indexes).
var_dump(function_exists('STRLEN'), function_exists('\\strlen'), function_exists('Str_Replace'));
var_dump(function_exists('strlen_'), function_exists(''), is_callable('ARRAY_MAP'));
var_dump(class_exists('arrayobject'), class_exists('\\ArrayObject'), interface_exists('TRAVERSABLE'));
var_dump(class_exists('ArrayObjec'), enum_exists('roundingmode'), trait_exists('ArrayObject'));
// Hierarchies of internal classes (computed on first use).
var_dump(new ArrayIterator([]) instanceof SeekableIterator);
var_dump(is_subclass_of('RecursiveArrayIterator', 'ArrayIterator'));
var_dump(is_a('UnexpectedValueException', 'RuntimeException', true));
var_dump(is_a('UnexpectedValueException', 'LogicException', true));
var_dump(class_implements('ArrayObject'));
var_dump(class_parents('UnexpectedValueException'));
var_dump((new ReflectionClass('RecursiveArrayIterator'))->getInterfaceNames());
var_dump((new ReflectionClass('ArrayIterator'))->getParentClass());
var_dump(get_parent_class(new LengthException()));
// Arginfo (built on first use) in errors and reflection.
try {
    strlen();
} catch (ArgumentCountError $e) {
    echo $e->getMessage(), "\n";
}
try {
    str_repeat([], 1);
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
try {
    STR_REPEAT('x', 'y');
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
foreach ((new ReflectionFunction('array_slice'))->getParameters() as $p) {
    echo $p, "\n";
}
echo (new ReflectionFunction('Json_Encode'))->getNumberOfRequiredParameters(), "\n";
var_dump(array_map(fn ($p) => $p->getName(), (new ReflectionFunction('preg_match'))->getParameters()));
// A user function declared later: the builtins' arginfo is still found.
eval('function late_user_fn($a) { return $a; }');
var_dump(late_user_fn(3));
try {
    strlen([]);
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
echo intdiv(7, 2), "\n";
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(false)
bool(false)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
array(5) {
  ["IteratorAggregate"]=>
  string(17) "IteratorAggregate"
  ["Traversable"]=>
  string(11) "Traversable"
  ["ArrayAccess"]=>
  string(11) "ArrayAccess"
  ["Serializable"]=>
  string(12) "Serializable"
  ["Countable"]=>
  string(9) "Countable"
}
array(2) {
  ["RuntimeException"]=>
  string(16) "RuntimeException"
  ["Exception"]=>
  string(9) "Exception"
}
array(7) {
  [0]=>
  string(9) "Countable"
  [1]=>
  string(12) "Serializable"
  [2]=>
  string(11) "ArrayAccess"
  [3]=>
  string(8) "Iterator"
  [4]=>
  string(11) "Traversable"
  [5]=>
  string(16) "SeekableIterator"
  [6]=>
  string(17) "RecursiveIterator"
}
bool(false)
string(14) "LogicException"
strlen() expects exactly 1 argument, 0 given
str_repeat(): Argument #1 ($string) must be of type string, array given
str_repeat(): Argument #2 ($times) must be of type int, string given
Parameter #0 [ <required> array $array ]
Parameter #1 [ <required> int $offset ]
Parameter #2 [ <optional> ?int $length = null ]
Parameter #3 [ <optional> bool $preserve_keys = false ]
1
array(5) {
  [0]=>
  string(7) "pattern"
  [1]=>
  string(7) "subject"
  [2]=>
  string(7) "matches"
  [3]=>
  string(5) "flags"
  [4]=>
  string(6) "offset"
}
int(3)
strlen(): Argument #1 ($string) must be of type string, array given
3
