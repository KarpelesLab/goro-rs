--TEST--
ucwords() delimiter test after uppercasing; strspn()/strcspn() null length; strtr() arity/type; html_entity_decode() charset lookup
--FILE--
<?php
// ucwords() tests the delimiter after uppercasing the previous byte
var_dump(ucwords('abc', 'a,b,,c'), ucwords('hello world-foo', '-'), ucwords('aaa', 'a'), ucwords('', 'x'), ucwords("x\ty z"));
// strspn()/strcspn(): a null length means no limit
var_dump(strspn('abc', 'abc', 1, null), strcspn('1.5', 'aBc', 0, null), strspn('aab', 'a', 0, null), strcspn('abcd', 'd', -3, null));
// strtr(): argument count and the 2-argument form's array type
foreach ([fn() => strtr('', ''), fn() => strtr('abc'), fn() => strtr('a', 'b', 'c', 'd'), fn() => strtr('abc', null), fn() => strtr('abc', ['a' => 'x'])] as $f) {
    try { var_dump($f()); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
}
// html_entity_decode() looks the charset up only when there is an entity
var_dump(html_entity_decode('no entities', ENT_QUOTES, 'bogus'), html_entity_decode('&amp;', ENT_QUOTES, 'bogus'));
--EXPECTF--
string(3) "AbC"
string(15) "Hello world-Foo"
string(3) "AaA"
string(0) ""
string(5) "X	Y Z"
int(2)
int(3)
int(2)
int(2)
TypeError: strtr(): Argument #2 ($from) must be of type array, string given
ArgumentCountError: strtr() expects at least 2 arguments, 1 given
ArgumentCountError: strtr() expects at most 3 arguments, 4 given
TypeError: strtr(): Argument #2 ($from) must be of type array, null given
string(3) "xbc"

Warning: html_entity_decode(): Charset "bogus" is not supported, assuming UTF-8 in %s on line 11
string(11) "no entities"
string(1) "&"
