--TEST--
Class table lookups and class-relation answers follow the table as it changes (aliases, late declarations, static writes, many classes)
--FILE--
<?php
interface IA {}
interface IB extends IA {}
class Base { public static $n = 0; public static $arr = []; }
class Kid extends Base implements IB {}

function check($o, $what) {
    echo $what, ': ';
    foreach (['IA', 'IB', 'Base', 'Kid', 'Late', 'LateI', 'AliasOfBase', 'AliasOfIA'] as $c) {
        echo $c, '=', ($o instanceof $c) ? 1 : 0, ' ';
    }
    echo "\n";
}

$k = new Kid;
check($k, 'start');
// static property writes between the checks
for ($i = 0; $i < 5; $i++) {
    Kid::$n++;
    Base::$arr[] = $i;
    check($k, "static $i");
}
var_dump(Base::$n, Kid::$n, count(Kid::$arr));

// an alias made after the answers were first given
class_alias('Base', 'AliasOfBase');
class_alias('IA', 'AliasOfIA');
check($k, 'aliases');
var_dump(is_a($k, 'aliasofbase'), is_subclass_of($k, 'ALIASOFIA'), $k instanceof AliasOfBase);

// classes declared later, conditionally
if ($k) {
    interface LateI extends IB {}
    class Late extends Kid implements LateI {}
}
check($k, 'late (Kid)');
$l = new Late;
check($l, 'late (Late)');
Late::$n = 42;
var_dump(Base::$n, Late::$n);
check($l, 'after static write');

// type checks
function takesIA(IA $x) { return get_class($x); }
function takesAlias(AliasOfBase $x) { return get_class($x); }
var_dump(takesIA($l), takesAlias($l), takesAlias($k));
try {
    takesIA(new Base);
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}

// instanceof never autoloads; is_a() with a string does
spl_autoload_register(function ($c) {
    echo "autoload($c)\n";
    if (strtolower($c) === 'auto1') {
        eval('class Auto1 extends Kid {}');
    }
});
var_dump($k instanceof Auto1);
var_dump(is_a('Auto1', 'Kid', true));
var_dump(is_a('auto1', 'IA', true), (new auto1) instanceof IB, (new AUTO1) instanceof aliasofia);
var_dump(is_a('Missing1', 'Kid', true));

// anonymous classes
$a = new class extends Kid implements Countable { function count(): int { return 3; } };
var_dump($a instanceof IA, $a instanceof Countable, $a instanceof Late, count($a));
$b = new class { };
var_dump($b instanceof IA, get_parent_class($a), get_parent_class($b));

// case variants
var_dump(get_class(new KID), get_class(new kid), new BASE instanceof base, class_exists('lAtE'));

// self / parent / static
class P1 { public static $v = 'p1'; static function who() { return [self::$v, static::$v, static::class]; } }
class P2 extends P1 { public static $v = 'p2'; static function up() { return parent::who(); } }
var_dump(P2::who(), P2::up());
P2::$v = 'changed';
var_dump(P2::who(), P1::who());

// many classes (several names per lookup cache slot)
$names = [];
for ($i = 0; $i < 3000; $i++) {
    $names[] = $n = "Many_Class_Number_$i";
    $parent = $i % 7 ? 'Kid' : 'Base';
    eval("class $n extends $parent { public static \$id = $i; }");
}
$bad = 0;
foreach ($names as $i => $n) {
    $u = strtoupper($n);
    if (!class_exists($u, false) || get_parent_class($u) !== ($i % 7 ? 'Kid' : 'Base')
        || $u::$id !== $i || (new $n instanceof IB) !== ($i % 7 != 0)) {
        $bad++;
    }
    $u::$id = -$i;
}
foreach ($names as $i => $n) {
    if ($n::$id !== -$i || !is_subclass_of($n, 'Base')) {
        $bad++;
    }
}
var_dump($bad, class_exists('Many_Class_Number_3000', false));
?>
--EXPECTF--
start: IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=0 AliasOfIA=0 
static 0: IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=0 AliasOfIA=0 
static 1: IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=0 AliasOfIA=0 
static 2: IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=0 AliasOfIA=0 
static 3: IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=0 AliasOfIA=0 
static 4: IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=0 AliasOfIA=0 
int(5)
int(5)
int(5)
aliases: IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=1 AliasOfIA=1 
bool(true)
bool(true)
bool(true)
late (Kid): IA=1 IB=1 Base=1 Kid=1 Late=0 LateI=0 AliasOfBase=1 AliasOfIA=1 
late (Late): IA=1 IB=1 Base=1 Kid=1 Late=1 LateI=1 AliasOfBase=1 AliasOfIA=1 
int(42)
int(42)
after static write: IA=1 IB=1 Base=1 Kid=1 Late=1 LateI=1 AliasOfBase=1 AliasOfIA=1 
string(4) "Late"
string(4) "Late"
string(3) "Kid"
takesIA(): Argument #1 ($x) must be of type IA, Base given, called in %s on line %d
bool(false)
autoload(Auto1)
bool(true)
bool(true)
bool(true)
bool(true)
autoload(Missing1)
bool(false)
bool(true)
bool(true)
bool(false)
int(3)
bool(false)
string(3) "Kid"
bool(false)
string(3) "Kid"
string(3) "Kid"
bool(true)
bool(true)
array(3) {
  [0]=>
  string(2) "p1"
  [1]=>
  string(2) "p2"
  [2]=>
  string(2) "P2"
}
array(3) {
  [0]=>
  string(2) "p1"
  [1]=>
  string(2) "p2"
  [2]=>
  string(2) "P2"
}
array(3) {
  [0]=>
  string(2) "p1"
  [1]=>
  string(7) "changed"
  [2]=>
  string(2) "P2"
}
array(3) {
  [0]=>
  string(2) "p1"
  [1]=>
  string(2) "p1"
  [2]=>
  string(2) "P1"
}
int(0)
bool(false)
