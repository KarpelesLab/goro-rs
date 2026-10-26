--TEST--
SPL: isset()/empty() on an object-backed ArrayObject see declared property slots as set and empty
--INI--
zend.exception_ignore_args=0
zend.exception_string_param_max_len=15
--FILE--
<?php
class P { public int $t; public ?int $n = null; public $u; }
$p = new P; unset($p->u);
$ao = new ArrayObject($p);
var_dump(isset($ao['t']), empty($ao['t']), $ao->offsetExists('t'), array_key_exists('t', (array)$p));
var_dump(isset($ao['n']), empty($ao['n']), $ao->offsetExists('n'));
var_dump(isset($ao['u']), empty($ao['u']), $ao->offsetExists('u'));
var_dump($ao['t'] ?? 'dflt');
var_dump(count($ao));
class P2 { public $a = 1; public $z = 0; public $s = 'x'; }
$p = new P2; $p->dyn = 1; $p->dz = 0; $p->dn = null;
$ao = @new ArrayObject($p);
foreach (['a', 'z', 's', 'dyn', 'dz', 'dn', 'missing'] as $k) {
    echo $k, ': ', json_encode([isset($ao[$k]), empty($ao[$k]), $ao->offsetExists($k)]), "\n";
}
$ai = @new ArrayIterator($p);
echo json_encode([isset($ai['a']), empty($ai['a']), isset($ai['z']), empty($ai['z'])]), "\n";
?>
--EXPECTF--

Deprecated: ArrayObject::__construct(): Using an object as a backing array for ArrayObject is deprecated, as it allows violating class constraints and invariants in %s on line 4
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
string(4) "dflt"
int(1)

Deprecated: Creation of dynamic property P2::$dyn is deprecated in %s on line 11

Deprecated: Creation of dynamic property P2::$dz is deprecated in %s on line 11

Deprecated: Creation of dynamic property P2::$dn is deprecated in %s on line 11
a: [true,true,true]
z: [true,true,true]
s: [true,true,true]
dyn: [true,false,true]
dz: [true,true,true]
dn: [false,true,true]
missing: [false,true,false]
[true,true,true,true]
