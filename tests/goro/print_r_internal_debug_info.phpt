--TEST--
print_r() shows internal classes' debug info (WeakMap, WeakReference, Generator, random engines)
--FILE--
<?php
$o = new stdClass;
$w = new WeakMap; $w[$o] = 1;
print_r($w); echo "\n";
print_r(WeakReference::create($o)); echo "\n";
print_r([new ArrayObject([1])]); echo "\n";
echo print_r(new ArrayObject(['a' => 1]), true), "\n";
$s = new SplObjectStorage; $s->attach($o, 'data');
echo print_r([$s], true), "\n";
print_r(new DateTimeImmutable('2020-01-01')); echo "\n";
print_r(new ReflectionClass('stdClass')); echo "\n";
print_r(new ReflectionMethod('ArrayObject', 'count')); echo "\n";
print_r((function() { yield 1; })()); echo "\n";
print_r(new SplTempFileObject()); echo "\n";
print_r(new SplQueue); echo "\n";
print_r(new ArrayIterator(new ArrayObject([1]))); echo "\n";
print_r(new RecursiveArrayIterator([1, [2]])); echo "\n";
print_r(new CachingIterator(new ArrayIterator([1]))); echo "\n";
print_r(new LimitIterator(new ArrayIterator([1]), 0, 1)); echo "\n";
print_r(new SplPriorityQueue); echo "\n";
class D2 extends DateTime { public $extra = 5; }
print_r(new D2('2020-01-01')); echo "\n";
class AO2 extends ArrayObject { private $priv = 1; }
print_r(new AO2([3])); echo "\n";
debug_zval_refcount: ;
var_export(new SplObjectStorage); echo "\n";
var_dump((array) new ArrayObject([1]));
print_r(new DateTimeZone('+02:00')); echo "\n";
print_r(new DateInterval('P1Y')); echo "\n";
$di = (new DateTime('2020-01-01'))->diff(new DateTime('2021-02-03')); print_r($di); echo "\n";
print_r(new Exception('m')); echo "\n";
var_dump(new Random\Engine\Xoshiro256StarStar(1));
var_dump(new Random\Engine\PcgOneseq128XslRr64(1));
print_r(new Random\Engine\Xoshiro256StarStar(1)); echo "\n";
print_r(new Random\Engine\PcgOneseq128XslRr64(1)); echo "\n";
var_export(new Random\Engine\Xoshiro256StarStar(1)); echo "\n";
var_dump((array)new Random\Engine\Xoshiro256StarStar(1));
--EXPECTF--
WeakMap Object
(
    [0] => Array
        (
            [key] => stdClass Object
                (
                )

            [value] => 1
        )

)

WeakReference Object
(
    [object] => stdClass Object
        (
        )

)

Array
(
    [0] => ArrayObject Object
        (
            [storage:ArrayObject:private] => Array
                (
                    [0] => 1
                )

        )

)

ArrayObject Object
(
    [storage:ArrayObject:private] => Array
        (
            [a] => 1
        )

)


Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 8
Array
(
    [0] => SplObjectStorage Object
        (
            [storage:SplObjectStorage:private] => Array
                (
                    [0] => Array
                        (
                            [obj] => stdClass Object
                                (
                                )

                            [inf] => data
                        )

                )

        )

)

DateTimeImmutable Object
(
    [date] => 2020-01-01 00:00:00.000000
    [timezone_type] => 3
    [timezone] => UTC
)

ReflectionClass Object
(
    [name] => stdClass
)

ReflectionMethod Object
(
    [name] => count
    [class] => ArrayObject
)

Generator Object
(
    [function] => {closure:%s:13}
)

SplTempFileObject Object
(
    [pathName:SplFileInfo:private] => php://temp
    [fileName:SplFileInfo:private] => php://temp
    [openMode:SplFileObject:private] => wb
    [delimiter:SplFileObject:private] => ,
    [enclosure:SplFileObject:private] => "
)

SplQueue Object
(
    [flags:SplDoublyLinkedList:private] => 4
    [dllist:SplDoublyLinkedList:private] => Array
        (
        )

)


Deprecated: ArrayIterator::__construct(): Using an object as a backing array for ArrayIterator is deprecated, as it allows violating class constraints and invariants in %s on line 16
ArrayIterator Object
(
    [storage:ArrayIterator:private] => ArrayObject Object
        (
            [storage:ArrayObject:private] => Array
                (
                    [0] => 1
                )

        )

)

RecursiveArrayIterator Object
(
    [storage:ArrayIterator:private] => Array
        (
            [0] => 1
            [1] => Array
                (
                    [0] => 2
                )

        )

)

CachingIterator Object
(
)

LimitIterator Object
(
)

SplPriorityQueue Object
(
    [flags:SplPriorityQueue:private] => 1
    [isCorrupted:SplPriorityQueue:private] => 
    [heap:SplPriorityQueue:private] => Array
        (
        )

)

D2 Object
(
    [extra] => 5
    [date] => 2020-01-01 00:00:00.000000
    [timezone_type] => 3
    [timezone] => UTC
)

AO2 Object
(
    [priv:AO2:private] => 1
    [storage:ArrayObject:private] => Array
        (
            [0] => 3
        )

)

\SplObjectStorage::__set_state(array(
))
array(1) {
  [0]=>
  int(1)
}
DateTimeZone Object
(
    [timezone_type] => 1
    [timezone] => +02:00
)

DateInterval Object
(
    [y] => 1
    [m] => 0
    [d] => 0
    [h] => 0
    [i] => 0
    [s] => 0
    [f] => 0
    [invert] => 0
    [days] => 
    [from_string] => 
)

DateInterval Object
(
    [y] => 1
    [m] => 1
    [d] => 2
    [h] => 0
    [i] => 0
    [s] => 0
    [f] => 0
    [invert] => 0
    [days] => 399
    [from_string] => 
)

Exception Object
(
    [message:protected] => m
    [string:Exception:private] => 
    [code:protected] => 0
    [file:protected] => %s
    [line:protected] => 31
    [trace:Exception:private] => Array
        (
        )

    [previous:Exception:private] => 
)

object(Random\Engine\Xoshiro256StarStar)#%d (1) {
  ["__states"]=>
  array(4) {
    [0]=>
    string(16) "c15c0289ec2d0a91"
    [1]=>
    string(16) "67ec8e65a18debbe"
    [2]=>
    string(16) "5e5532fbeea293f8"
    [3]=>
    string(16) "0bc942ee9086c171"
  }
}
object(Random\Engine\PcgOneseq128XslRr64)#%d (1) {
  ["__states"]=>
  array(2) {
    [0]=>
    string(16) "9cd108b9ceabd26b"
    [1]=>
    string(16) "df3b50d88069a5ef"
  }
}
Random\Engine\Xoshiro256StarStar Object
(
    [__states] => Array
        (
            [0] => c15c0289ec2d0a91
            [1] => 67ec8e65a18debbe
            [2] => 5e5532fbeea293f8
            [3] => 0bc942ee9086c171
        )

)

Random\Engine\PcgOneseq128XslRr64 Object
(
    [__states] => Array
        (
            [0] => 9cd108b9ceabd26b
            [1] => df3b50d88069a5ef
        )

)

\Random\Engine\Xoshiro256StarStar::__set_state(array(
))
array(0) {
}
