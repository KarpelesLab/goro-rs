--TEST--
print_r() of an SplFixedArray shows its properties
--FILE--
<?php
class F extends SplFixedArray { public $q = 1; private $r = 2; }
$f = new F(2); $f[0] = [1]; $f->p = 'a';
print_r($f); echo "\n";
$g = new SplFixedArray(2); $g[1] = new F(1); $g->{'5'} = 'x';
print_r($g); echo "\n";
print_r([$g]);
--EXPECTF--

Deprecated: Creation of dynamic property F::$p is deprecated in %s on line 3
F Object
(
    [0] => Array
        (
            [0] => 1
        )

    [1] => 
    [q] => 1
    [r:F:private] => 2
    [p] => a
)


Deprecated: Creation of dynamic property SplFixedArray::$5 is deprecated in %s on line 5
SplFixedArray Object
(
    [0] => 
    [1] => F Object
        (
            [0] => 
            [q] => 1
            [r:F:private] => 2
        )

    [5] => x
)

Array
(
    [0] => SplFixedArray Object
        (
            [0] => 
            [1] => F Object
                (
                    [0] => 
                    [q] => 1
                    [r:F:private] => 2
                )

            [5] => x
        )

)
