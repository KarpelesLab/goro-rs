--TEST--
CG(zend_lineno) after a closure is its closing line (yield from, strlen...); ReflectionGenerator of a delegating generator
--FILE--
<?php
function g() {
    yield from (function () {
        yield 1;
        throw new Exception("inner");
    })();
}
try { foreach (g() as $v) {} } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
function g2() {
    yield from new ArrayIterator(
        [1]
    );
    yield from
       5;
}
try { foreach (g2() as $v) {} } catch (Throwable $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
function g3() {
    $x = yield (function () {
        return 1;
    })();
    echo strlen(function () {
    });
}
try { foreach (g3() as $v) {} } catch (Throwable $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
$g = (function () { yield from (function () { yield 1; })(
); })();
$g->current();
$r = new ReflectionGenerator($g);
var_dump($r->getExecutingLine());

function call(ReflectionGenerator $ref, $method, $rec = true) {
    if ($rec) {
        call($ref, $method, false);
        return;
    }
    var_dump($ref->$method());
}

function doCalls(ReflectionGenerator $ref) {
    call($ref, "getTrace");
    call($ref, "getExecutingLine");
    call($ref, "getExecutingFile");
    call($ref, "getExecutingGenerator");
    call($ref, "getFunction");
    call($ref, "getThis");
}

($gen = (function() use (&$gen) {
    $ref = new ReflectionGenerator($gen);

    doCalls($ref);

    yield from (function() use ($ref) {
        doCalls($ref);
        yield; // Generator !
    })();
})())->next();



function t2($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; } }
t2(function () {
    $a = [] +
      1;
});
t2(function () {
    $a = [
       function () {
          return 1;
       }, intdiv(1, 0)];
});
t2(function () {
    $x = (function () {
        return 1;
    }) + 1;
});
t2(function () {
    $x = [function () {
        return 1;
    }] + 1;
});
t2(function () {
    $x = strlen(function () {
        return 1;
    });
});
t2(function () {
    $x = (fn() =>
        1
    ) . "";
});
t2(function () {
    $g = (function () { yield 1; })();
    foreach ($g as $v) {}
    $g->rewind();
    $x = new ArrayIterator(function () {
    });
});
t2(function () {
    $x = 1 + match(true) {
        false => 1,
    };
});
t2(function () {
    $x = [1, 2][
        5
    ];
    echo $undefined
      . "x";
    echo "end\n";
});
t2(function () {
    throw new Exception(
        "multi"
    );
});
t2(function () {
    $o = new class {
        function f() {}
    };
    $o->g(function () {
    });
});
t2(function () {
    $a = function () {
    } instanceof Closure ?: undefined_fn();
    undefined_fn2(function () {
    });
});
--EXPECTF--
#0 %s(6): {closure:g():3}()
#1 %s(8): g()
#2 {main}
Can use "yield from" only with arrays and Traversables @14
strlen(): Argument #1 ($string) must be of type string, Closure given @22
int(25)
array(1) {
  [0]=>
  array(2) {
    ["function"]=>
    string(%d) "{closure:%s:48}"
    ["args"]=>
    array(0) {
    }
  }
}
int(51)
string(%d) "%s"
object(Generator)#8 (1) {
  ["function"]=>
  string(%d) "{closure:%s:48}"
}
object(ReflectionFunction)#10 (1) {
  ["name"]=>
  string(%d) "{closure:%s:48}"
}
NULL
array(2) {
  [0]=>
  array(4) {
    ["file"]=>
    string(%d) "%s"
    ["line"]=>
    int(56)
    ["function"]=>
    string(%d) "{closure:{closure:%s:48}:53}"
    ["args"]=>
    array(0) {
    }
  }
  [1]=>
  array(2) {
    ["function"]=>
    string(%d) "{closure:%s:48}"
    ["args"]=>
    array(0) {
    }
  }
}
int(56)
string(%d) "%s"
object(Generator)#11 (1) {
  ["function"]=>
  string(%d) "{closure:{closure:%s:48}:53}"
}
object(ReflectionFunction)#12 (1) {
  ["name"]=>
  string(%d) "{closure:%s:48}"
}
NULL
TypeError: Unsupported operand types: array + int @64
DivisionByZeroError: Division by zero @70
TypeError: Unsupported operand types: Closure + int @75
TypeError: Unsupported operand types: array + int @80
TypeError: strlen(): Argument #1 ($string) must be of type string, Closure given @85
Error: Object of class Closure could not be converted to string @90
Exception: Cannot rewind a generator that was already run @95
UnhandledMatchError: Unhandled match case true @100

Warning: Undefined array key 5 in %s on line 106

Warning: Undefined variable $undefined in %s on line 109
xend
Exception: multi @113
Error: Call to undefined method class@anonymous::g() @121
Error: Call to undefined function undefined_fn2() @127
