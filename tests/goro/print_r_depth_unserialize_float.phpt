--TEST--
print_r() of deep (non-recursive) nesting; unserialize() widens ints for float properties
--FILE--
<?php
$a = []; for ($i = 0; $i < 40; $i++) $a = [$a];
print_r($a);
echo strlen(print_r($a, true)), "\n";
$o = new stdClass; $c = $o; for ($i = 0; $i < 30; $i++) { $c->n = new stdClass; $c = $c->n; }
echo strlen(print_r($o, true)), "\n";
$r = [1]; $r[] = &$r; print_r($r); echo "\n";
class T { public float $f = 0.0; public float $g = 1.0; public ?float $h = null; public float|string $u = 0.0; public int|float $w = 0; }
var_dump(unserialize('O:1:"T":1:{s:1:"f";i:5;}'));
var_dump(unserialize('O:1:"T":3:{s:1:"h";i:7;s:1:"u";i:8;s:1:"w";i:9;}'));
var_dump(unserialize('O:1:"T":2:{s:1:"f";i:5;s:1:"g";R:2;}'));
--EXPECT--
Array
(
    [0] => Array
        (
            [0] => Array
                (
                    [0] => Array
                        (
                            [0] => Array
                                (
                                    [0] => Array
                                        (
                                            [0] => Array
                                                (
                                                    [0] => Array
                                                        (
                                                            [0] => Array
                                                                (
                                                                    [0] => Array
                                                                        (
                                                                            [0] => Array
                                                                                (
                                                                                    [0] => Array
                                                                                        (
                                                                                            [0] => Array
                                                                                                (
                                                                                                    [0] => Array
                                                                                                        (
                                                                                                            [0] => Array
                                                                                                                (
                                                                                                                    [0] => Array
                                                                                                                        (
                                                                                                                            [0] => Array
                                                                                                                                (
                                                                                                                                    [0] => Array
                                                                                                                                        (
                                                                                                                                            [0] => Array
                                                                                                                                                (
                                                                                                                                                    [0] => Array
                                                                                                                                                        (
                                                                                                                                                            [0] => Array
                                                                                                                                                                (
                                                                                                                                                                    [0] => Array
                                                                                                                                                                        (
                                                                                                                                                                            [0] => Array
                                                                                                                                                                                (
                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                        (
                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                (
                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                        (
                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                (
                                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                                        (
                                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                                (
                                                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                                                        (
                                                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                                                (
                                                                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                                                                        (
                                                                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                                                                (
                                                                                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                                                                                        (
                                                                                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                                                                                (
                                                                                                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                                                                                                        (
                                                                                                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                                                                                                (
                                                                                                                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                                                                                                                        (
                                                                                                                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                                                                                                                (
                                                                                                                                                                                                                                                                                                                    [0] => Array
                                                                                                                                                                                                                                                                                                                        (
                                                                                                                                                                                                                                                                                                                            [0] => Array
                                                                                                                                                                                                                                                                                                                                (
                                                                                                                                                                                                                                                                                                                                )

                                                                                                                                                                                                                                                                                                                        )

                                                                                                                                                                                                                                                                                                                )

                                                                                                                                                                                                                                                                                                        )

                                                                                                                                                                                                                                                                                                )

                                                                                                                                                                                                                                                                                        )

                                                                                                                                                                                                                                                                                )

                                                                                                                                                                                                                                                                        )

                                                                                                                                                                                                                                                                )

                                                                                                                                                                                                                                                        )

                                                                                                                                                                                                                                                )

                                                                                                                                                                                                                                        )

                                                                                                                                                                                                                                )

                                                                                                                                                                                                                        )

                                                                                                                                                                                                                )

                                                                                                                                                                                                        )

                                                                                                                                                                                                )

                                                                                                                                                                                        )

                                                                                                                                                                                )

                                                                                                                                                                        )

                                                                                                                                                                )

                                                                                                                                                        )

                                                                                                                                                )

                                                                                                                                        )

                                                                                                                                )

                                                                                                                        )

                                                                                                                )

                                                                                                        )

                                                                                                )

                                                                                        )

                                                                                )

                                                                        )

                                                                )

                                                        )

                                                )

                                        )

                                )

                        )

                )

        )

)
20250
11900
Array
(
    [0] => 1
    [1] => Array
 *RECURSION*
)

object(T)#32 (5) {
  ["f"]=>
  float(5)
  ["g"]=>
  float(1)
  ["h"]=>
  NULL
  ["u"]=>
  float(0)
  ["w"]=>
  int(0)
}
object(T)#32 (5) {
  ["f"]=>
  float(0)
  ["g"]=>
  float(1)
  ["h"]=>
  float(7)
  ["u"]=>
  float(8)
  ["w"]=>
  int(9)
}
object(T)#32 (5) {
  ["f"]=>
  &float(5)
  ["g"]=>
  &float(5)
  ["h"]=>
  NULL
  ["u"]=>
  float(0)
  ["w"]=>
  int(0)
}
