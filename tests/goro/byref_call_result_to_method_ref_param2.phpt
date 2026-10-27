--TEST--
Call results sent to by-value / by-reference parameters of methods and constructors: references kept or dropped, by-value arrays separated
--FILE--
<?php
class U {
    public array $sets = [5 => ['n' => 1]];
    public function & get(int $oid): array { return $this->sets[$oid]; }
    public function val(int $oid): array { return $this->sets[$oid]; }
    public static function & sget(U $u): array { return $u->sets[5]; }
}
class ByVal { public function __construct(array $a) { $a['n'] = 'changed'; } public function m(array $a) { $a['n'] = 'm'; return $a; } }
class ByRef { public array $r; public function __construct(array &$a) { $a['n'] = 'ctor'; } public function m(array &$a) { $a['n'] = 'mref'; } }
$u = new U; $x = [];
new ByVal($u->get(5)); var_dump($u->sets[5]['n']);
(new ByVal([]))->m($u->get(5)); var_dump($u->sets[5]['n']);
new ByRef($u->get(5)); var_dump($u->sets[5]['n']);
(new ByRef($x))->m($u->get(5)); var_dump($u->sets[5]['n']);
(new ByRef($x))->m(U::sget($u)); var_dump($u->sets[5]['n']);
new ByRef($u->val(5)); var_dump($u->sets[5]['n']);
$f = function (&$a) { $a['n'] = 'closure'; };
$f($u->get(5)); var_dump($u->sets[5]['n']);
var_dump(count($u->get(5)), array_keys($u->get(5)));

?>
--EXPECTF--
int(1)
int(1)
string(4) "ctor"
string(4) "mref"
string(4) "mref"

Notice: Only variables should be passed by reference in %s on line 16
string(4) "mref"
string(7) "closure"
int(1)
array(1) {
  [0]=>
  string(1) "n"
}
