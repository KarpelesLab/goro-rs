--TEST--
Deprecation messages name the declaring class; enum cases via constant(); internal declarations' #[\Deprecated] in Reflection; print_r() of a null __debugInfo()
--FILE--
<?php
class I { #[\Deprecated] const IC = 1; #[\Deprecated] static function s() { return 1; } }
class C2 extends I {
    static function t() { static::s(); self::s(); parent::s(); return static::IC + self::IC; }
}
var_dump(C2::IC);
var_dump(C2::t());
C2::s();
enum E {
    #[\Deprecated] case A;
    const K = self::A;
}
var_dump(E::K);
var_dump(constant('E::A'));
class Q {
    const X =
        I::IC;
}
var_dump(Q::X);
class A2 { function __debugInfo() { return null; } }
print_r(new A2); echo "\n";
var_dump(xml_parser_free(...) instanceof Closure);
foreach (['xml_parser_free', 'socket_set_timeout'] as $f) {
    if (!function_exists($f)) { echo "$f missing\n"; continue; }
    foreach ((new ReflectionFunction($f))->getAttributes() as $a) { echo $a->getName(), " ", json_encode($a->getArguments()), "\n"; }
}
foreach ((new ReflectionMethod('SplObjectStorage', 'attach'))->getAttributes() as $a) { echo $a->getName(), " ", json_encode($a->getArguments()), "\n"; }
var_dump((new ReflectionClassConstant('DateTimeInterface', 'RFC7231'))->isDeprecated());
var_dump((new ReflectionClassConstant('DateTime', 'RFC7231'))->isDeprecated());
foreach ((new ReflectionClassConstant('DateTimeInterface', 'RFC7231'))->getAttributes() as $a) { echo $a->getName(), " ", json_encode($a->getArguments()), "\n"; }
if (class_exists('PDO')) { $r = new ReflectionClassConstant('PDO', 'SQLITE_DETERMINISTIC'); var_dump($r->isDeprecated()); foreach ($r->getAttributes() as $a) { echo $a->getName(), " ", json_encode($a->getArguments()), "\n"; } var_dump(PDO::SQLITE_DETERMINISTIC); }
$c = function (int $a = null) {};
$d = fn (int $a = null) => 1;
#[\Deprecated(message: "a", since: "b", extra: 1)] function fx() {}
try { fx(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--

Deprecated: {closure:%s:32}(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 32

Deprecated: {closure:%s:33}(): Implicitly marking parameter $a as nullable is deprecated, the explicit nullable type must be used instead in %s on line 33

Deprecated: Constant I::IC is deprecated in %s on line 6
int(1)

Deprecated: Method I::s() is deprecated in %s on line 4

Deprecated: Method I::s() is deprecated in %s on line 4

Deprecated: Method I::s() is deprecated in %s on line 4

Deprecated: Constant I::IC is deprecated in %s on line 4

Deprecated: Constant I::IC is deprecated in %s on line 4
int(2)

Deprecated: Method I::s() is deprecated in %s on line 8

Deprecated: Enum case E::A is deprecated in %s on line 11
enum(E::A)

Deprecated: Enum case E::A is deprecated in %s on line 14
enum(E::A)

Deprecated: Constant I::IC is deprecated in %s on line 17
int(1)

Deprecated: Returning null from A2::__debugInfo() is deprecated, return an empty array instead in %s on line 21
A2 Object
(
)

bool(true)
Deprecated {"since":"8.5","message":"as it has no effect since PHP 8.0"}
Deprecated {"since":"8.5","message":"use stream_set_timeout() instead"}
Deprecated {"since":"8.5","message":"use method SplObjectStorage::offsetSet() instead"}
bool(true)
bool(true)
Deprecated {"since":"8.5","message":"as this format ignores the associated timezone and always uses GMT"}
bool(true)
Deprecated {"since":"8.5","message":"use Pdo\\Sqlite::DETERMINISTIC instead"}

Deprecated: Constant PDO::SQLITE_DETERMINISTIC is deprecated since 8.5, use Pdo\Sqlite::DETERMINISTIC instead in %s on line 31
int(2048)
Error: Unknown named parameter $extra
