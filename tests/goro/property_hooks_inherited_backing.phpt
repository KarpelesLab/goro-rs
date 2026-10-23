--TEST--
A property overriding a backed hooked property is backed; virtual properties have no default value
--FILE--
<?php
class P {
    public int $v { get => 1; }
    public int $b { get => $this->b; set => $value; }
    public int $c { set => $value * 2; }
    public $plain;
}
class Q extends P {
    public int $v { get => 2; }
    public int $c { get => 5; }
}
class R extends P {}
foreach (['P', 'Q', 'R'] as $cl) {
    foreach (['v', 'b', 'c', 'plain'] as $pn) {
        $rp = new ReflectionProperty($cl, $pn);
        echo "$cl::$pn virtual=", var_export($rp->isVirtual(), true), " hooks=", count($rp->getHooks()), " hasHooks=", var_export($rp->hasHooks(), true), " default=", var_export($rp->hasDefaultValue(), true), "\n";
    }
}
var_dump(get_class_vars('Q'));
var_dump((array)new Q);
class HP {
    public $v { get => 1; }
    public $w { get => 1; set { } }
    public $plain = 3;
    public $b { set => $value; }
}
class HQ extends HP {
    public $v { get => $this->v ?? 7; }
    public $w { get => 2; }
    public $plain { get => $this->plain * 2; }
    public $b { get => 9; }
}
class HR extends HQ {
    public $v { get => 5; }
}
foreach (['HP', 'HQ', 'HR'] as $cl) {
    foreach (['v', 'w', 'plain', 'b'] as $pn) {
        $rp = new ReflectionProperty($cl, $pn);
        echo "$cl::$pn virtual=", var_export($rp->isVirtual(), true), " hooks=", implode(',', array_keys($rp->getHooks())), " default=", var_export($rp->hasDefaultValue(), true), "\n";
    }
    var_dump(get_class_vars($cl));
}
$q = new HQ; var_dump($q->plain, $q->v); $q->b = 4; var_dump($q->b);
--EXPECTF--
P::v virtual=true hooks=1 hasHooks=true default=false
P::b virtual=false hooks=2 hasHooks=true default=false
P::c virtual=false hooks=1 hasHooks=true default=false
P::plain virtual=false hooks=0 hasHooks=false default=true
Q::v virtual=true hooks=1 hasHooks=true default=false
Q::b virtual=false hooks=2 hasHooks=true default=false
Q::c virtual=false hooks=2 hasHooks=true default=false
Q::plain virtual=false hooks=0 hasHooks=false default=true
R::v virtual=true hooks=1 hasHooks=true default=false
R::b virtual=false hooks=2 hasHooks=true default=false
R::c virtual=false hooks=1 hasHooks=true default=false
R::plain virtual=false hooks=0 hasHooks=false default=true
array(3) {
  ["c"]=>
  NULL
  ["b"]=>
  NULL
  ["plain"]=>
  NULL
}
array(1) {
  ["plain"]=>
  NULL
}
HP::v virtual=true hooks=get default=false
HP::w virtual=true hooks=get,set default=false
HP::plain virtual=false hooks= default=true
HP::b virtual=false hooks=set default=true
array(2) {
  ["plain"]=>
  int(3)
  ["b"]=>
  NULL
}
HQ::v virtual=false hooks=get default=true
HQ::w virtual=true hooks=get,set default=false
HQ::plain virtual=false hooks=get default=true
HQ::b virtual=false hooks=get,set default=true
array(3) {
  ["v"]=>
  NULL
  ["plain"]=>
  NULL
  ["b"]=>
  NULL
}
HR::v virtual=false hooks=get default=true
HR::w virtual=true hooks=get,set default=false
HR::plain virtual=false hooks=get default=true
HR::b virtual=false hooks=get,set default=true
array(3) {
  ["v"]=>
  NULL
  ["plain"]=>
  NULL
  ["b"]=>
  NULL
}
int(0)
int(7)
int(9)
