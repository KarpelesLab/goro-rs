--TEST--
parent::$x::get() of a private hooked parent property throws
--FILE--
<?php
class P {
    private $Xy { get => 1; }
    protected $z { get => 2; set { echo "P::set $value\n"; } }
    private $w = 5;
}
class C extends P {
    public $Xy { get => parent::$Xy::get(); }
    public $z { get => parent::$z::get(); set { parent::$z::set($value); } }
    public $w { get => parent::$w::get(); }
}
$c = new C;
try {
    var_dump($c->Xy);
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}
var_dump($c->z);
$c->z = 3;
try {
    var_dump($c->w);
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}
--EXPECT--
Cannot access private property P::$Xy
int(2)
P::set 3
Cannot access private property P::$w
