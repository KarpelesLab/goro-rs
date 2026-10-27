--TEST--
A by-reference method result passed to a reference parameter of a constructor/method (known at run time) passes the reference
--FILE--
<?php
class U {
    public array $sets = [5 => ['name' => ['a', 'b']]];
    public function & get(int $oid): array { $data = []; if (!isset($this->sets[$oid])) return $data; return $this->sets[$oid]; }
}
class Args {
    private array $cs;
    public function __construct(object $e, array &$changeSet) { $this->cs = &$changeSet; }
    public function set($f, $v) { $this->cs[$f][1] = $v; }
}
function takeRef(array &$a) { $a['name'][1] = 'viaFunc'; }
class Inv { public function invoke($x, Args $a) { $a->set('name', 'Bob'); } }
$u = new U;
$a = new Args($u, $u->get(5));
$a->set('name', 'Bob');
var_dump($u->sets[5]['name'][1]);
takeRef($u->get(5));
var_dump($u->sets[5]['name'][1]);
(new Inv)->invoke(1, new Args($u, $u->get(5)));
var_dump($u->sets[5]['name'][1]);
$m = 'get'; $b = new Args($u, $u->$m(5)); $b->set('name', 'dyn');
var_dump($u->sets[5]['name'][1]);

?>
--EXPECTF--
string(3) "Bob"
string(7) "viaFunc"
string(3) "Bob"
string(3) "dyn"
