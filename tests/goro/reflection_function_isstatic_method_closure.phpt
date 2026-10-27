--TEST--
ReflectionFunction::isStatic() of a closure of a static method
--FILE--
<?php
class C {
    static function getInstance() {}
    function inst() {}
    static function mk() { return static function () {}; }
    static function mk2() { return function () {}; }
    static function mk3() { return self::getInstance(...); }
}
$cases = [
    'static fcc' => C::getInstance(...),
    'inst fcc' => (new C)->inst(...),
    'fromCallable' => Closure::fromCallable(['C', 'getInstance']),
    'fromCallable str' => Closure::fromCallable('C::getInstance'),
    'static closure' => C::mk(),
    'closure in static' => C::mk2(),
    'self fcc' => C::mk3(),
    'strlen' => strlen(...),
    'plain' => function () {},
    'static plain' => static fn () => 1,
];
foreach ($cases as $k => $c) {
    $rf = new ReflectionFunction($c);
    echo $k, ": ", var_export($rf->isStatic(), true), " ", (str_starts_with($rf->name, "{closure") ? "{closure}" : $rf->name), "\n";
}

?>
--EXPECT--
static fcc: true getInstance
inst fcc: false inst
fromCallable: true getInstance
fromCallable str: true getInstance
static closure: true {closure}
closure in static: false {closure}
self fcc: true getInstance
strlen: false strlen
plain: false {closure}
static plain: true {closure}
