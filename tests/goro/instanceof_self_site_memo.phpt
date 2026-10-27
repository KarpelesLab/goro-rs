--TEST--
instanceof self run repeatedly: in methods, inherited methods, trait methods and closures bound to other scopes
--FILE--
<?php
trait T {
    function isSelf($o) { return $o instanceof self ? 'y' : 'n'; }
}
interface I {}
class A implements I {
    use T;
    function check($o) { return $o instanceof self ? 'Y' : 'N'; }
    static function closure() { return function ($o) { return $o instanceof self ? 'c' : '-'; }; }
}
class B extends A {}
class C { use T; }
$objs = [new A, new B, new C, new ArrayObject([]), null, 'A'];
$cl = A::closure();
$bound = Closure::bind($cl, null, C::class);
for ($i = 0; $i < 3; $i++) {
    $s = '';
    foreach ($objs as $o) {
        $s .= (new A)->check($o) . (new B)->check($o) . (new A)->isSelf($o) . (new C)->isSelf($o)
            . $cl($o) . $bound($o) . ' ';
    }
    echo rtrim($s), "\n";
}
?>
--EXPECT--
YYync- YYync- NNny-c NNnn-- NNnn-- NNnn--
YYync- YYync- NNny-c NNnn-- NNnn-- NNnn--
YYync- YYync- NNny-c NNnn-- NNnn-- NNnn--
