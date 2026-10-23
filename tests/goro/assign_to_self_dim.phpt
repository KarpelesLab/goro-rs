--TEST--
Assigning a variable to one of its own dimensions reads it first
--FILE--
<?php
// `$a[...] = $a` reads the value before the container is fetched for writing.
$a = [1]; $a[] = $a; var_dump($a);
$b = [1, [2]]; $b[1][] = $b; var_dump($b);
$c['k'][0] = $c; var_dump($c);
$d = 'str'; try { $d[0] = $d; } catch (Error $e) { echo $e->getMessage(), "\n"; } var_dump($d);
$o = new stdClass; $o->p = [1]; $o->p[] = $o; var_dump(count($o->p));
$r = [1]; $rr = &$r; $r[] = $r; var_dump($r);
$x = ['a' => 1]; $x['a'] = $x; var_dump($x);
function f() { $z['q'][] = $z; return $z; } var_dump(f());
$s = 5; try { $s[0] = $s; } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    [0]=>
    int(1)
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
    array(2) {
      [0]=>
      int(1)
      [1]=>
      array(1) {
        [0]=>
        int(2)
      }
    }
  }
}

Warning: Undefined variable $c in %s on line 5
array(1) {
  ["k"]=>
  array(1) {
    [0]=>
    NULL
  }
}

Warning: Only the first byte will be assigned to the string offset in %s on line 6
string(3) "str"
int(2)
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    [0]=>
    int(1)
  }
}
array(1) {
  ["a"]=>
  array(1) {
    ["a"]=>
    int(1)
  }
}

Warning: Undefined variable $z in %s on line 10
array(1) {
  ["q"]=>
  array(1) {
    [0]=>
    NULL
  }
}
Cannot use a scalar value as an array
