--TEST--
http_build_query(): references, recursion, accessible object properties, prefix/separator/encoding arguments
--FILE--
<?php
set_error_handler(function ($no, $msg) { echo "[$msg]\n"; return true; });
$b=[1,2]; $a=["x"=>&$b,"n"=>1]; $n=null; $a["y"]=&$n; echo http_build_query($a), "\n";
$a = ["x" => 1]; $a['self'] = &$a; echo http_build_query($a), "\n";
$a = ["x" => 1]; $a['c'] = $a; echo http_build_query($a), "\n";
class A { public $a=1; protected $b=2; private $c=3; function q(){return http_build_query($this);} }
class B extends A { private $c='bc'; public $d=4; function qb(){return http_build_query($this);} }
$o=new B; echo $o->q(), "\n", $o->qb(), "\n", http_build_query($o), "\n";
$o->dyn = 5; $o->{'0'} = 'z'; echo http_build_query($o), "\n";
echo http_build_query(['o' => $o]), "\n";
echo http_build_query(new ArrayObject([1,2])), "|\n";
echo http_build_query(new DateTime('2020-01-01')), "|\n";
function gen() { yield 1; }
echo http_build_query(gen()), "|\n";
echo http_build_query(['d' => new DateTime('2020-01-01'), 'e' => 1]), "|\n";
echo http_build_query((object)['1' => 'a', 'b' => 'c'], 'pre'), "\n";
echo http_build_query([1=>['x'=>1], 2 => 'v', 'k' => [3 => 4]], 'p~ '), "\n";
echo http_build_query(['a b' => 'c d~', 'e~' => ['f g' => 'h~']], '', '&', PHP_QUERY_RFC3986), "\n";
echo http_build_query(['a b' => 'c d~'], '', '&', 0), "\n";
echo http_build_query(['a b' => 'c d~'], '', '&', -1), "\n";
echo http_build_query(['a b' => 'c d~'], '', '&', 1), "\n";
echo http_build_query(['a' => 1, 'b' => 2], '', ''), "\n";
echo http_build_query(['a' => 1, 'b' => 2], '', null), "\n";
ini_set('arg_separator.output', ';;');
echo http_build_query(['a' => 1, 'b' => 2]), "\n";
echo http_build_query(['a' => 1, 'b' => 2], '', ''), "\n";
echo http_build_query(['a' => 1, 'b' => ['c' => 2]], '', '|'), "\n";
ini_set('arg_separator.output', '');
echo http_build_query(['a' => 1, 'b' => 2]), "\n";
try { http_build_query([], []); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { http_build_query([], "", []); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { http_build_query([], "", "&", "x"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { http_build_query("x"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo http_build_query([5 => 1], null), "\n";
echo http_build_query(['n' => NAN, 'i' => INF, 'f' => 0.1, 'g' => -1.5e30, 't' => true, 'fa' => false, 'r' => STDIN, 'nl' => null]), "\n";
echo http_build_query(["a\0b" => "\xff\x00"]), "\n";
enum E: string { case A = 'x y'; }
enum U { case A; }
echo http_build_query(['e' => E::A]), "\n";
try { echo http_build_query(['e' => U::A, 'z' => 1]), "\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
class H { public int $u; public $v = 1; public $w { get => 5; } public $x = 2 { get => $this->x * 10; } }
echo http_build_query(new H), "\n";
--EXPECT--
x%5B0%5D=1&x%5B1%5D=2&n=1
x=1
x=1&c%5Bx%5D=1
a=1&b=2&c=3&d=4
a=1&b=2&c=bc&d=4
a=1&d=4
[Creation of dynamic property B::$dyn is deprecated]
[Creation of dynamic property B::$0 is deprecated]
a=1&d=4&dyn=5&0=z
o%5Ba%5D=1&o%5Bd%5D=4&o%5Bdyn%5D=5&o%5B0%5D=z
|
|
|
e=1|
1=a&b=c
p~ 1%5Bx%5D=1&p~ 2=v&k%5B3%5D=4
a%20b=c%20d~&e~%5Bf%20g%5D=h~
a+b=c+d%7E
a+b=c+d%7E
a+b=c+d%7E
a=1b=2
a=1&b=2
a=1;;b=2
a=1b=2
a=1|b%5Bc%5D=2
a=1;;b=2
http_build_query(): Argument #2 ($numeric_prefix) must be of type string, array given
http_build_query(): Argument #3 ($arg_separator) must be of type ?string, array given
http_build_query(): Argument #4 ($encoding_type) must be of type int, string given
http_build_query(): Argument #1 ($data) must be of type array, string given
[http_build_query(): Passing null to parameter #2 ($numeric_prefix) of type string is deprecated]
5=1
[unexpected NAN value was coerced to string]
n=NAN;;i=INF;;f=0.1;;g=-1.5E%2B30;;t=1;;fa=0
a%00b=%FF%00
e=x+y
Unbacked enum U cannot be converted to a string
v=1;;x=2
