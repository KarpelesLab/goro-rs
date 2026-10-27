--TEST--
new of internal classes (and user subclasses) from a site run many times
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
class MyEx extends RuntimeException {}
class MyEx2 extends Exception { public $extra = [1, 2]; protected $code = 5; }
class MyErr extends Error { public $p = 'x'; }

function mk($i) {
    return new Exception("m$i", $i);
}
function mk2($i, $prev) {
    return new MyEx("e$i", $i, $prev);
}
for ($i = 0; $i < 3; $i++) {
    $e = mk($i);
    echo get_class($e), ' ', $e->getMessage(), ' ', $e->getCode(), ' ', $e->getLine(), ' ', count($e->getTrace()), "\n";
    echo $e->getTrace()[0]['function'], ' ', json_encode($e->getTrace()[0]['args']), "\n";
    $f = mk2($i, $e);
    echo get_class($f), ' ', $f->getMessage(), ' ', $f->getCode(), ' ', get_class($f->getPrevious()), ' ', $f->getLine(), "\n";
    $g = new MyEx2();
    echo $g->getCode(), ' ', json_encode($g->extra), ' ', $g->getLine(), "\n";
    $g->extra[] = $i;
    $h = new MyErr("err", 7);
    echo $h->getMessage(), $h->getCode(), $h->p, ' ', $h->getLine(), "\n";
    $x = new ErrorException("ee", 1, E_WARNING, "f.php", 12);
    echo $x->getSeverity(), $x->getFile(), $x->getLine(), "\n";
    $t = new TypeError();
    echo json_encode($t->getMessage()), ' ', $t->getLine(), "\n";
    $d = new Exception(42, 3);
    var_dump($d->getMessage());
    try {
        new Exception([]);
    } catch (TypeError $te) {
        echo $te->getMessage(), "\n";
    }
    try {
        new Exception("a", "b");
    } catch (TypeError $te) {
        echo $te->getMessage(), "\n";
    }
    try {
        new Exception(message: "named", code: $i);
    } catch (Throwable $te) {
        echo get_class($te), "\n";
    }
    $n = new Exception(message: "named", code: $i);
    echo $n->getMessage(), $n->getCode(), "\n";
    $ao = new ArrayObject([1, 2, $i]);
    echo count($ao), ' ';
    $ao[] = 5;
    $ao2 = new ArrayObject();
    echo count($ao2), ' ', count($ao), "\n";
    $s = new SplStack();
    $s->push($i);
    $s2 = new SplStack();
    echo count($s), count($s2), "\n";
    $os = new SplObjectStorage();
    $os[new stdClass] = 1;
    echo count($os), count(new SplObjectStorage()), "\n";
    $dt = new DateTime('@' . (86400 * $i));
    echo $dt->format('Y-m-d'), "\n";
    $dti = new DateTimeImmutable('2020-01-0' . ($i + 1));
    echo $dti->format('Y-m-d'), "\n";
    try {
        new DateTime('garbage-date');
    } catch (Exception $de) {
        echo get_class($de), ': ', $de->getMessage(), "\n";
    }
    $rc = new ReflectionClass('stdClass');
    echo $rc->getName(), "\n";
    $doc = new DOMDocument();
    $doc->loadXML("<a>$i</a>");
    echo $doc->documentElement->textContent, "\n";
    var_dump(new ArrayIterator([$i]));
}
echo $e, "\n";
?>
--EXPECTF--
Exception m0 0 7 1
mk [0]
MyEx e0 0 Exception 10
5 [1,2] 18
err7x 21
2f.php12
"" 25
string(2) "42"
Exception::__construct(): Argument #1 ($message) must be of type string, array given
Exception::__construct(): Argument #2 ($code) must be of type int, string given
named0
3 0 4
10
10
1970-01-01
2020-01-01
DateMalformedStringException: Failed to parse time string (garbage-date) at position 0 (g): The timezone could not be found in the database
stdClass
0
object(ArrayIterator)#21 (1) {
  ["storage":"ArrayIterator":private]=>
  array(1) {
    [0]=>
    int(0)
  }
}
Exception m1 1 7 1
mk [1]
MyEx e1 1 Exception 10
5 [1,2] 18
err7x 21
2f.php12
"" 25
string(2) "42"
Exception::__construct(): Argument #1 ($message) must be of type string, array given
Exception::__construct(): Argument #2 ($code) must be of type int, string given
named1
3 0 4
10
10
1970-01-02
2020-01-02
DateMalformedStringException: Failed to parse time string (garbage-date) at position 0 (g): The timezone could not be found in the database
stdClass
1
object(ArrayIterator)#20 (1) {
  ["storage":"ArrayIterator":private]=>
  array(1) {
    [0]=>
    int(1)
  }
}
Exception m2 2 7 1
mk [2]
MyEx e2 2 Exception 10
5 [1,2] 18
err7x 21
2f.php12
"" 25
string(2) "42"
Exception::__construct(): Argument #1 ($message) must be of type string, array given
Exception::__construct(): Argument #2 ($code) must be of type int, string given
named2
3 0 4
10
10
1970-01-03
2020-01-03
DateMalformedStringException: Failed to parse time string (garbage-date) at position 0 (g): The timezone could not be found in the database
stdClass
2
object(ArrayIterator)#18 (1) {
  ["storage":"ArrayIterator":private]=>
  array(1) {
    [0]=>
    int(2)
  }
}
Exception: m2 in %s:7
Stack trace:
#0 %s(13): mk(2)
#1 {main}
