--TEST--
Array comparison casts elements (__toString) only up to the first unequal pair
--FILE--
<?php
class S { function __construct(public $n) {} function __toString(): string { echo "ts{$this->n} "; return "x{$this->n}"; } }
echo "gt: "; var_dump(new S(1) > "a");
echo "gt2: "; var_dump("a" > new S(2));
echo "ge: "; var_dump(new S(1) >= "a");
echo "lt: "; var_dump(new S(1) < "a");
echo "arr gt: "; var_dump([new S(1), "b"] > ["a", new S(2)]);
echo "arr lt: "; var_dump([new S(1), "b"] < ["a", new S(2)]);
echo "num: "; var_dump(new stdClass > 1);
echo "num2: "; var_dump(1 > new stdClass);
echo "two: "; var_dump([new stdClass, 1] > [2, new stdClass]);
echo "two lt: "; var_dump([new stdClass, 1] < [2, new stdClass]);
function a() { echo "A "; return 1; } function b() { echo "B "; return 2; }
echo "eval: "; var_dump(a() > b());
echo "1: "; var_dump([new S(1), new S(2)] == ["x1", "x2"]);
echo "2: "; var_dump([new S(1), new S(2)] == ["x9", "x2"]);
echo "3: "; var_dump([new S(1), new S(2)] < ["x1", "x3"]);
echo "4: "; var_dump(["a" => new S(1), "b" => new S(2)] == ["b" => "x2", "a" => "x1"]);
echo "5: "; var_dump(["a" => new S(1), "b" => new S(2)] == ["c" => "x2", "a" => "x1"]);
echo "6: "; var_dump([new S(1), new S(2)] == ["x1"]);
echo "7: "; var_dump([new stdClass, new stdClass] == [1, 2]);
echo "8: "; var_dump([[new S(1)], new S(2)] == [["x0"], "x2"]);
echo "9: "; var_dump([[new S(1)], new S(2)] <=> [["x1"], "x3"]);
echo "10: "; var_dump([1, new S(2)] > [2, "x2"]);
--EXPECTF--
gt: ts1 bool(true)
gt2: ts2 bool(false)
ge: ts1 bool(true)
lt: ts1 bool(false)
arr gt: ts1 bool(true)
arr lt: ts1 bool(false)
num: 
Notice: Object of class stdClass could not be converted to int in %s on line 9
bool(false)
num2: 
Notice: Object of class stdClass could not be converted to int in %s on line 10
bool(false)
two: 
Notice: Object of class stdClass could not be converted to int in %s on line 11
bool(false)
two lt: 
Notice: Object of class stdClass could not be converted to int in %s on line 12
bool(true)
eval: A B bool(false)
1: ts1 ts2 bool(true)
2: ts1 bool(false)
3: ts1 ts2 bool(true)
4: ts1 ts2 bool(true)
5: ts1 bool(false)
6: bool(false)
7: 
Notice: Object of class stdClass could not be converted to int in %s on line 21

Notice: Object of class stdClass could not be converted to int in %s on line 21
bool(false)
8: ts1 bool(false)
9: ts1 ts2 int(-1)
10: bool(false)
