--TEST--
Writes to holders of a shared literal array separate it
--FILE--
<?php
function lit() { return [3, 1, 2, 'k' => [9, 8]]; }
for ($i = 0; $i < 2; $i++) {
    foreach ([3, 1, 2] as &$v) { $v *= 10; } unset($v);
    foreach ([[1], [2]] as $k => &$v) { $v[] = $k; } unset($v);
    $x = lit(); $x[] = 4; $x['k'][] = 7; sort($x['k']);
    $y = lit(); $r = &$y['k'][0]; $r = 'R'; unset($r);
    $z = lit(); array_push($z['k'], 6); array_walk($z, function (&$e) { if (is_int($e)) $e++; });
    $o = new stdClass; $o->p = [1, [2]]; $o->p[1][] = 3; $o->q = [1, [2]]; $o->q[] = 5;
    $s = [5, 4]; $t = &$s; sort($t);
    $u = [1, 2]; $u2 = &$u[1]; $u2 = 'X';
    var_dump(lit(), $x, $y, $z, $o, $s, $u);
    $c = function ($a = [1, [2]]) { $a[1][] = 3; return $a; };
    var_dump($c(), $c());
    $g = (function () { $a = yield [1, 2]; $a[] = 1; yield $a; })();
    $first = $g->current(); $first[] = 9; $g->send([7]); var_dump($first, $g->current(), [1, 2]);
    $arr = [1, 2]; $it = new ArrayIterator([1, 2]); $it[] = 3; var_dump(iterator_to_array($it), [1, 2]);
    $ao = new ArrayObject([1, 2]); $ao->append(3); $ao->asort(); var_dump($ao->getArrayCopy(), [1, 2]);
    $l = [1, 2]; list($p, $q) = [1, 2]; [$m, [$n]] = [1, [2]]; var_dump($p, $q, $m, $n);
    $w = ['a' => 1]; $w2 = $w; $w2['a']++; var_dump($w, $w2, ['a' => 1]);
    $e = []; $e[] = 1; $e2 = []; var_dump($e, $e2, []);
    $st = static function () { static $s = [1]; $s[] = count($s); return $s; }; $st(); var_dump($st());
    $ref = [1, 2]; $rr = &$ref; $rr[] = 3; var_dump($ref, [1, 2]);
    $n2 = [1, 2]; next($n2); var_dump(current($n2), current([1, 2]), key([1, 2]));
    $sp = [...[1, 2], ...[3]]; $sp[] = 4; var_dump($sp, [...[1, 2], ...[3]]);
    $big = array_merge([1, 2], [3]); $big[] = 9; var_dump(array_merge([1, 2], [3]));
    $ser = unserialize(serialize([1, [2]])); $ser[1][] = 3; var_dump([1, [2]]);
    $ex = ['a' => 1, 'b' => 2]; extract($ex, EXTR_REFS); $a = 'A'; var_dump($ex, ['a' => 1, 'b' => 2]);
    $cm = compact('i'); var_dump($cm);
    $sh = [1, 2, 3]; array_shift($sh); array_unshift($sh, 0); array_splice($sh, 1, 1, ['S']); var_dump($sh, [1, 2, 3]);
    $str = ['x' => 'abc']; $str['x'][0] = 'Z'; var_dump($str, ['x' => 'abc']);
}
--EXPECT--
array(4) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  ["k"]=>
  array(2) {
    [0]=>
    int(9)
    [1]=>
    int(8)
  }
}
array(5) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  ["k"]=>
  array(3) {
    [0]=>
    int(7)
    [1]=>
    int(8)
    [2]=>
    int(9)
  }
  [3]=>
  int(4)
}
array(4) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  ["k"]=>
  array(2) {
    [0]=>
    string(1) "R"
    [1]=>
    int(8)
  }
}
array(4) {
  [0]=>
  int(4)
  [1]=>
  int(2)
  [2]=>
  int(3)
  ["k"]=>
  array(3) {
    [0]=>
    int(9)
    [1]=>
    int(8)
    [2]=>
    int(6)
  }
}
object(stdClass)#1 (2) {
  ["p"]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    array(2) {
      [0]=>
      int(2)
      [1]=>
      int(3)
    }
  }
  ["q"]=>
  array(3) {
    [0]=>
    int(1)
    [1]=>
    array(1) {
      [0]=>
      int(2)
    }
    [2]=>
    int(5)
  }
}
array(2) {
  [0]=>
  int(4)
  [1]=>
  int(5)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  &string(1) "X"
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(2) {
    [0]=>
    int(2)
    [1]=>
    int(3)
  }
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(2) {
    [0]=>
    int(2)
    [1]=>
    int(3)
  }
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(9)
}
array(2) {
  [0]=>
  int(7)
  [1]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
int(1)
int(2)
int(1)
int(2)
array(1) {
  ["a"]=>
  int(1)
}
array(1) {
  ["a"]=>
  int(2)
}
array(1) {
  ["a"]=>
  int(1)
}
array(1) {
  [0]=>
  int(1)
}
array(0) {
}
array(0) {
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
int(2)
int(1)
int(0)
array(4) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
  [3]=>
  int(4)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    [0]=>
    int(2)
  }
}
array(2) {
  ["a"]=>
  &string(1) "A"
  ["b"]=>
  &int(2)
}
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
array(1) {
  ["i"]=>
  int(0)
}
array(3) {
  [0]=>
  int(0)
  [1]=>
  string(1) "S"
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(1) {
  ["x"]=>
  string(3) "Zbc"
}
array(1) {
  ["x"]=>
  string(3) "abc"
}
array(4) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  ["k"]=>
  array(2) {
    [0]=>
    int(9)
    [1]=>
    int(8)
  }
}
array(5) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  ["k"]=>
  array(3) {
    [0]=>
    int(7)
    [1]=>
    int(8)
    [2]=>
    int(9)
  }
  [3]=>
  int(4)
}
array(4) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  ["k"]=>
  array(2) {
    [0]=>
    string(1) "R"
    [1]=>
    int(8)
  }
}
array(4) {
  [0]=>
  int(4)
  [1]=>
  int(2)
  [2]=>
  int(3)
  ["k"]=>
  array(3) {
    [0]=>
    int(9)
    [1]=>
    int(8)
    [2]=>
    int(6)
  }
}
object(stdClass)#8 (2) {
  ["p"]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    array(2) {
      [0]=>
      int(2)
      [1]=>
      int(3)
    }
  }
  ["q"]=>
  array(3) {
    [0]=>
    int(1)
    [1]=>
    array(1) {
      [0]=>
      int(2)
    }
    [2]=>
    int(5)
  }
}
array(2) {
  [0]=>
  int(4)
  [1]=>
  int(5)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  &string(1) "X"
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(2) {
    [0]=>
    int(2)
    [1]=>
    int(3)
  }
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(2) {
    [0]=>
    int(2)
    [1]=>
    int(3)
  }
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(9)
}
array(2) {
  [0]=>
  int(7)
  [1]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
int(1)
int(2)
int(1)
int(2)
array(1) {
  ["a"]=>
  int(1)
}
array(1) {
  ["a"]=>
  int(2)
}
array(1) {
  ["a"]=>
  int(1)
}
array(1) {
  [0]=>
  int(1)
}
array(0) {
}
array(0) {
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
int(2)
int(1)
int(0)
array(4) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
  [3]=>
  int(4)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    [0]=>
    int(2)
  }
}
array(2) {
  ["a"]=>
  &string(1) "A"
  ["b"]=>
  &int(2)
}
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
array(1) {
  ["i"]=>
  int(1)
}
array(3) {
  [0]=>
  int(0)
  [1]=>
  string(1) "S"
  [2]=>
  int(3)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(1) {
  ["x"]=>
  string(3) "Zbc"
}
array(1) {
  ["x"]=>
  string(3) "abc"
}
