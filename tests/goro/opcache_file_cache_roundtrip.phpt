--TEST--
opcache.file_cache: included files run the same from the file cache (classes, closures, statics, attributes), corrupt and stale entries are compiled again
--FILE--
<?php
$dir = sys_get_temp_dir() . '/goro_fc_' . getmypid();
@mkdir("$dir/cache", 0777, true);
file_put_contents("$dir/a.php", <<<'PHP'
<?php
namespace Fc;
const X = [1, 'a' => 2.5, 'b' => [true, null], -3 => "s"];
#[\Attribute]
class Attr { public function __construct(public $v = PHP_INT_MAX) {} }
interface I { const C = 'c'; }
trait T { public function hello() { static $n = 0; return __CLASS__ . ++$n; } }
/** Base doc */
#[Attr(5)]
abstract class Base implements I {
    abstract function f(int|string $x): ?array;
    public $p = self::C . 'x';
    public function __get($n) { return "magic $n"; }
}
final class K extends Base {
    use T;
    function f(int|string $x = 3): ?array { return [$x, fn($y) => $y * 2]; }
    public static $s = [1, 2];
    public readonly int $r;
    public function __construct() { $this->r = 7; }
}
enum E: string {
    case A = 'a';
    case B = 'b';
    const D = self::B;
    public function label(): string { return ucfirst($this->value); }
}
function gen() { yield 1; yield from [2, 3]; }
function counter() { static $c = 0; return ++$c; }
static $top = 0;
$top++;
$cl = function ($a) use (&$top) { return $a + $top; };
$anon = new class { public function x() { return 'anon'; } };
return [$cl, fn($x) => $x * 2, strlen(...), $anon];
PHP);
file_put_contents("$dir/b.php", <<<'PHP'
<?php
function b_closures() {
    return [function () { static $s = 10; return ++$s; }, fn() => __FUNCTION__];
}
return 'b ' . __LINE__;
PHP);
file_put_contents("$dir/main.php", <<<'PHP'
<?php
[$cl, $arrow, $fcc, $anon] = require __DIR__ . '/a.php';
echo require __DIR__ . '/b.php', "\n";
[$c1, $c2] = b_closures();
$own = function () { return 'own'; };
var_dump(\Fc\X, $cl(1), $arrow(3), $fcc("abcd"), $anon->x(), $own());
var_dump($c1(), $c1(), $c2());
$k = new Fc\K;
var_dump($k->hello(), $k->hello(), $k->f()[1](4), $k->p, $k->zz, $k->r, Fc\K::$s);
var_dump(Fc\E::from('b')->label(), Fc\E::D, Fc\E::cases());
foreach (Fc\gen() as $v) echo $v;
echo Fc\counter(), Fc\counter(), "\n";
$r = new ReflectionClass('Fc\Base');
var_dump($r->getDocComment());
foreach ($r->getAttributes() as $a) {
    var_dump($a->getName(), $a->getArguments(), $a->newInstance());
}
echo $r->getMethod('f'), new ReflectionMethod('Fc\K', 'f');
var_dump(opcache_is_script_cached_in_file_cache(__DIR__ . '/a.php'));
PHP);
foreach (['a.php', 'b.php', 'main.php'] as $f) {
    touch("$dir/$f", time() - 100);
}
$run = function () use ($dir) {
    $cmd = [PHP_BINARY, '-n', '-d', 'opcache.enable_cli=1', '-d', "opcache.file_cache=$dir/cache",
        '-d', 'opcache.file_cache_only=1', "$dir/main.php"];
    $p = proc_open($cmd, [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    $out = stream_get_contents($pipes[1]) . stream_get_contents($pipes[2]);
    proc_close($p);
    return $out;
};
$bins = fn() => glob("$dir/cache/*$dir/*.php.bin");
$cold = $run();
echo $cold;
var_dump(count($bins()));
$warm = $run();
var_dump($warm === $cold);
// (the entry is used while the file keeps its mtime and size)
$t = filemtime("$dir/b.php");
$src = file_get_contents("$dir/b.php");
file_put_contents("$dir/b.php", str_replace("'b '", "'B '", $src));
touch("$dir/b.php", $t);
var_dump($run() === $cold);
file_put_contents("$dir/b.php", $src);
touch("$dir/b.php", $t);
// A truncated entry and a foreign one are compiled again (and rewritten).
foreach ($bins() as $i => $bin) {
    file_put_contents($bin, $i ? 'garbage' : substr(file_get_contents($bin), 0, 100));
}
var_dump($run() === $cold);
var_dump($run() === $cold);
// A changed file is compiled again.
file_put_contents("$dir/b.php", "<?php function b_closures() { return [fn() => 1, fn() => 2]; }\nreturn 'changed';");
touch("$dir/b.php", time() - 50);
$out = $run();
var_dump(str_contains($out, 'changed'), $run() === $out);
$it = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS),
    RecursiveIteratorIterator::CHILD_FIRST);
foreach ($it as $f) {
    $f->isDir() ? rmdir($f->getPathname()) : unlink($f->getPathname());
}
rmdir($dir);
?>
--EXPECTF--
b 5
array(4) {
  [0]=>
  int(1)
  ["a"]=>
  float(2.5)
  ["b"]=>
  array(2) {
    [0]=>
    bool(true)
    [1]=>
    NULL
  }
  [-3]=>
  string(1) "s"
}
int(2)
int(6)
int(4)
string(4) "anon"
string(3) "own"
int(11)
int(12)
string(24) "{closure:b_closures():3}"
string(5) "Fc\K1"
string(5) "Fc\K2"
int(8)
string(2) "cx"
string(8) "magic zz"
int(7)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
string(1) "B"
enum(Fc\E::B)
array(2) {
  [0]=>
  enum(Fc\E::A)
  [1]=>
  enum(Fc\E::B)
}
12312
string(15) "/** Base doc */"
string(7) "Fc\Attr"
array(1) {
  [0]=>
  int(5)
}
object(Fc\Attr)#%d (1) {
  ["v"]=>
  int(5)
}
Method [ <user> abstract public method f ] {
  @@ %sa.php 11 - 11

  - Parameters [1] {
    Parameter #0 [ <required> string|int $x ]
  }
  - Return [ ?array ]
}
Method [ <user, overwrites Fc\Base, prototype Fc\Base> public method f ] {
  @@ %sa.php 17 - 17

  - Parameters [1] {
    Parameter #0 [ <optional> string|int $x = 3 ]
  }
  - Return [ ?array ]
}
bool(true)
int(2)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
