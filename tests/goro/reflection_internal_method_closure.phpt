--TEST--
ReflectionFunction/ReflectionMethod of closures of internal methods and functions
--FILE--
<?php
// ReflectionFunction / ReflectionMethod of closures of internal methods and
// functions.
function info($c) {
    $r = new ReflectionFunction($c);
    echo $r->getName(), " int=", var_export($r->isInternal(), true), " np=", $r->getNumberOfParameters(),
        " req=", $r->getNumberOfRequiredParameters(),
        " scope=", var_export($r->getClosureScopeClass()?->name, true),
        " this=", is_object($r->getClosureThis()) ? get_class($r->getClosureThis()) : 'null',
        " called=", var_export($r->getClosureCalledClass()?->name, true),
        " static=", var_export($r->isStatic(), true),
        " closure=", var_export($r->isClosure(), true),
        " anon=", var_export($r->isAnonymous(), true),
        " ext=", var_export($r->getExtensionName(), true),
        " ret=", var_export($r->hasReturnType() ? (string)$r->getReturnType() : null, true),
        " tent=", var_export($r->hasTentativeReturnType() ? (string)$r->getTentativeReturnType() : null, true),
        "\n";
    foreach ($r->getParameters() as $p) {
        echo "  ", $p, " ", $p->getDeclaringFunction()->getName(), " ", get_class($p->getDeclaringFunction()), "\n";
    }
    var_dump($r->getStaticVariables(), $r->getClosureUsedVariables(), $r->getClosure() === $c, $r);
    echo $r;
    $m = new ReflectionMethod($c, '__invoke');
    var_dump($m->class, $m->name, $m->getNumberOfParameters(), $m->isStatic());
    echo $m;
}
$ao = new ArrayObject([1, 2, 3]);
$cases = [
    'fcc offsetGet' => $ao->offsetGet(...),
    'fromCallable offsetGet' => Closure::fromCallable([$ao, "offsetGet"]),
    'getClosure count' => (new ReflectionMethod('ArrayObject', 'count'))->getClosure($ao),
    'fcc static' => DateTime::createFromFormat(...),
    'fromCallable static' => Closure::fromCallable('DateTime::createFromFormat'),
    'getClosure static' => (new ReflectionMethod('DateTime', 'createFromFormat'))->getClosure(),
    'strlen' => strlen(...),
    'fromCallable sort' => Closure::fromCallable('sort'),
    'fcc format' => (new DateTime("2020-01-01"))->format(...),
    'fcc getIterator' => $ao->getIterator(...),
];
foreach ($cases as $k => $c) {
    echo "== $k\n";
    info($c);
}
$c = (new ReflectionMethod('ArrayObject', 'count'))->getClosure($ao);
$g = (new ReflectionMethod('ArrayObject', 'offsetGet'))->getClosure($ao);
var_dump($c(), (new ReflectionFunction($c))->invoke(), (new ReflectionFunction($c))->invokeArgs([]));
var_dump((new ReflectionFunction($g))->invoke(1), (new ReflectionFunction($g))->invokeArgs(['key' => 0]), (new ReflectionFunction($g))->invoke(key: 2));
try { (new ReflectionFunction($g))->invoke(nope: 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump((new ReflectionMethod($g, '__invoke'))->invoke($g, 1));
try { (new ReflectionMethod('ArrayObject', 'count'))->getClosure(new stdClass); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$s = strlen(...);
var_dump((new ReflectionMethod($s, '__invoke'))->invoke($s, 'abc'));
echo new ReflectionFunction(SplFixedArray::fromArray(...));
--EXPECT--
== fcc offsetGet
offsetGet int=true np=1 req=1 scope='ArrayObject' this=ArrayObject called='ArrayObject' static=false closure=true anon=false ext='SPL' ret=NULL tent='mixed'
  Parameter #0 [ <required> mixed $key ] offsetGet ReflectionMethod
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#13 (1) {
  ["name"]=>
  string(9) "offsetGet"
}
Closure [ <internal:SPL, prototype ArrayAccess> public method offsetGet ] {

  - Parameters [1] {
    Parameter #0 [ <required> mixed $key ]
  }
  - Tentative return [ mixed ]
}
string(7) "Closure"
string(8) "__invoke"
int(1)
bool(false)
Method [ <internal, prototype ArrayAccess> public method __invoke ] {

  - Parameters [1] {
    Parameter #0 [ <required> mixed $key ]
  }
  - Tentative return [ mixed ]
}
== fromCallable offsetGet
offsetGet int=true np=1 req=1 scope='ArrayObject' this=ArrayObject called='ArrayObject' static=false closure=true anon=false ext='SPL' ret=NULL tent='mixed'
  Parameter #0 [ <required> mixed $key ] offsetGet ReflectionMethod
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#15 (1) {
  ["name"]=>
  string(9) "offsetGet"
}
Closure [ <internal:SPL, prototype ArrayAccess> public method offsetGet ] {

  - Parameters [1] {
    Parameter #0 [ <required> mixed $key ]
  }
  - Tentative return [ mixed ]
}
string(7) "Closure"
string(8) "__invoke"
int(1)
bool(false)
Method [ <internal, prototype ArrayAccess> public method __invoke ] {

  - Parameters [1] {
    Parameter #0 [ <required> mixed $key ]
  }
  - Tentative return [ mixed ]
}
== getClosure count
count int=true np=0 req=0 scope='ArrayObject' this=ArrayObject called='ArrayObject' static=false closure=true anon=false ext='SPL' ret=NULL tent='int'
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#13 (1) {
  ["name"]=>
  string(5) "count"
}
Closure [ <internal:SPL, prototype Countable> public method count ] {

  - Parameters [0] {
  }
  - Tentative return [ int ]
}
string(7) "Closure"
string(8) "__invoke"
int(0)
bool(false)
Method [ <internal, prototype Countable> public method __invoke ] {

  - Parameters [0] {
  }
  - Tentative return [ int ]
}
== fcc static
createFromFormat int=true np=3 req=2 scope='DateTime' this=null called='DateTime' static=true closure=true anon=false ext='date' ret=NULL tent='DateTime|false'
  Parameter #0 [ <required> string $format ] createFromFormat ReflectionMethod
  Parameter #1 [ <required> string $datetime ] createFromFormat ReflectionMethod
  Parameter #2 [ <optional> ?DateTimeZone $timezone = null ] createFromFormat ReflectionMethod
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#14 (1) {
  ["name"]=>
  string(16) "createFromFormat"
}
Closure [ <internal:date> static public method createFromFormat ] {

  - Parameters [3] {
    Parameter #0 [ <required> string $format ]
    Parameter #1 [ <required> string $datetime ]
    Parameter #2 [ <optional> ?DateTimeZone $timezone = null ]
  }
  - Tentative return [ DateTime|false ]
}
string(7) "Closure"
string(8) "__invoke"
int(3)
bool(false)
Method [ <internal> public method __invoke ] {

  - Parameters [3] {
    Parameter #0 [ <required> string $format ]
    Parameter #1 [ <required> string $datetime ]
    Parameter #2 [ <optional> ?DateTimeZone $timezone = null ]
  }
  - Tentative return [ DateTime|false ]
}
== fromCallable static
createFromFormat int=true np=3 req=2 scope='DateTime' this=null called='DateTime' static=true closure=true anon=false ext='date' ret=NULL tent='DateTime|false'
  Parameter #0 [ <required> string $format ] createFromFormat ReflectionMethod
  Parameter #1 [ <required> string $datetime ] createFromFormat ReflectionMethod
  Parameter #2 [ <optional> ?DateTimeZone $timezone = null ] createFromFormat ReflectionMethod
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#15 (1) {
  ["name"]=>
  string(16) "createFromFormat"
}
Closure [ <internal:date> static public method createFromFormat ] {

  - Parameters [3] {
    Parameter #0 [ <required> string $format ]
    Parameter #1 [ <required> string $datetime ]
    Parameter #2 [ <optional> ?DateTimeZone $timezone = null ]
  }
  - Tentative return [ DateTime|false ]
}
string(7) "Closure"
string(8) "__invoke"
int(3)
bool(false)
Method [ <internal> public method __invoke ] {

  - Parameters [3] {
    Parameter #0 [ <required> string $format ]
    Parameter #1 [ <required> string $datetime ]
    Parameter #2 [ <optional> ?DateTimeZone $timezone = null ]
  }
  - Tentative return [ DateTime|false ]
}
== getClosure static
createFromFormat int=true np=3 req=2 scope='DateTime' this=null called='DateTime' static=true closure=true anon=false ext='date' ret=NULL tent='DateTime|false'
  Parameter #0 [ <required> string $format ] createFromFormat ReflectionMethod
  Parameter #1 [ <required> string $datetime ] createFromFormat ReflectionMethod
  Parameter #2 [ <optional> ?DateTimeZone $timezone = null ] createFromFormat ReflectionMethod
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#14 (1) {
  ["name"]=>
  string(16) "createFromFormat"
}
Closure [ <internal:date> static public method createFromFormat ] {

  - Parameters [3] {
    Parameter #0 [ <required> string $format ]
    Parameter #1 [ <required> string $datetime ]
    Parameter #2 [ <optional> ?DateTimeZone $timezone = null ]
  }
  - Tentative return [ DateTime|false ]
}
string(7) "Closure"
string(8) "__invoke"
int(3)
bool(false)
Method [ <internal> public method __invoke ] {

  - Parameters [3] {
    Parameter #0 [ <required> string $format ]
    Parameter #1 [ <required> string $datetime ]
    Parameter #2 [ <optional> ?DateTimeZone $timezone = null ]
  }
  - Tentative return [ DateTime|false ]
}
== strlen
strlen int=true np=1 req=1 scope=NULL this=null called=NULL static=false closure=true anon=false ext='Core' ret='int' tent=NULL
  Parameter #0 [ <required> string $string ] strlen ReflectionFunction
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#15 (1) {
  ["name"]=>
  string(6) "strlen"
}
Closure [ <internal:Core> function strlen ] {

  - Parameters [1] {
    Parameter #0 [ <required> string $string ]
  }
  - Return [ int ]
}
string(7) "Closure"
string(8) "__invoke"
int(1)
bool(false)
Method [ <internal> public method __invoke ] {

  - Parameters [1] {
    Parameter #0 [ <required> string $string ]
  }
  - Return [ int ]
}
== fromCallable sort
sort int=true np=2 req=1 scope=NULL this=null called=NULL static=false closure=true anon=false ext='standard' ret='true' tent=NULL
  Parameter #0 [ <required> array &$array ] sort ReflectionFunction
  Parameter #1 [ <optional> int $flags = SORT_REGULAR ] sort ReflectionFunction
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#14 (1) {
  ["name"]=>
  string(4) "sort"
}
Closure [ <internal:standard> function sort ] {

  - Parameters [2] {
    Parameter #0 [ <required> array &$array ]
    Parameter #1 [ <optional> int $flags = SORT_REGULAR ]
  }
  - Return [ true ]
}
string(7) "Closure"
string(8) "__invoke"
int(2)
bool(false)
Method [ <internal> public method __invoke ] {

  - Parameters [2] {
    Parameter #0 [ <required> array &$array ]
    Parameter #1 [ <optional> int $flags = SORT_REGULAR ]
  }
  - Return [ true ]
}
== fcc format
format int=true np=1 req=1 scope='DateTime' this=DateTime called='DateTime' static=false closure=true anon=false ext='date' ret=NULL tent='string'
  Parameter #0 [ <required> string $format ] format ReflectionMethod
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#16 (1) {
  ["name"]=>
  string(6) "format"
}
Closure [ <internal:date, prototype DateTimeInterface> public method format ] {

  - Parameters [1] {
    Parameter #0 [ <required> string $format ]
  }
  - Tentative return [ string ]
}
string(7) "Closure"
string(8) "__invoke"
int(1)
bool(false)
Method [ <internal, prototype DateTimeInterface> public method __invoke ] {

  - Parameters [1] {
    Parameter #0 [ <required> string $format ]
  }
  - Tentative return [ string ]
}
== fcc getIterator
getIterator int=true np=0 req=0 scope='ArrayObject' this=ArrayObject called='ArrayObject' static=false closure=true anon=false ext='SPL' ret=NULL tent='Iterator'
array(0) {
}
array(0) {
}
bool(true)
object(ReflectionFunction)#14 (1) {
  ["name"]=>
  string(11) "getIterator"
}
Closure [ <internal:SPL, prototype IteratorAggregate> public method getIterator ] {

  - Parameters [0] {
  }
  - Tentative return [ Iterator ]
}
string(7) "Closure"
string(8) "__invoke"
int(0)
bool(false)
Method [ <internal, prototype IteratorAggregate> public method __invoke ] {

  - Parameters [0] {
  }
  - Tentative return [ Iterator ]
}
int(3)
int(3)
int(3)
int(2)
int(1)
int(3)
Error: Unknown named parameter $nope
int(2)
ReflectionException: Given object is not an instance of the class this method was declared in
int(3)
Closure [ <internal:SPL> static public method fromArray ] {

  - Parameters [2] {
    Parameter #0 [ <required> array $array ]
    Parameter #1 [ <optional> bool $preserveKeys = true ]
  }
  - Tentative return [ SplFixedArray ]
}
