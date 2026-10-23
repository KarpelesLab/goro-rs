--TEST--
Temporaries are released by the opcode consuming them, and on unwinding in Zend order
--FILE--
<?php
class D {
    public function __construct(public string $n) {}
    public function __destruct() { echo "~{$this->n}\n"; }
}
function arr(...$a) { return $a; }
function show($v) { echo "show ", is_array($v) ? count($v) : $v, "\n"; }

// A temporary array is released by the op reading it.
show(arr(new D('a'), new D('b'))[0]->n);
echo "--\n";
// An array operand of + is released by the addition.
show([0] + [new D('c')]);
echo "--\n";
// The operand of a compound assignment goes after the write.
$o = new stdClass;
$o->p = [0];
$o->p += [new D('d')];
echo "after d\n";
$a = [[0]];
$a[0] += [new D('e')];
echo "after e\n";
// A temporary consumed by a comparison.
var_dump(arr(new D('f')) == []);
echo "--\n";
// Object ids: the released temporaries' ids are reused.
$x = arr(new stdClass, new stdClass)[1];
var_dump(spl_object_id($x), spl_object_id(new stdClass));
// Unwinding: an interrupted call's arguments go before the frame's temporaries.
function g() { return [new D('h'), (function () { throw new Exception('x'); })()]; }
try {
    arr(new D('i'), g());
} catch (Exception $e) {
    echo "caught\n";
}
// A destructor throwing while a try's live temporaries are released
// replaces the exception being caught.
class T { function __destruct() { throw new Exception('dtor'); } }
try {
    [new T, (function () { throw new LogicException('first'); })()];
} catch (Exception $e) {
    echo get_class($e), ' ', $e->getMessage(), ' prev ', get_class($e->getPrevious()), "\n";
}
echo "done\n";
?>
--EXPECT--
~b
~a
show a
--
~c
show 1
--
~d
after d
~e
after e
~f
bool(false)
--
int(3)
int(2)
~h
~i
caught
Exception dtor prev LogicException
done
