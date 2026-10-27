--TEST--
array &$array builtins called by a namespaced name over 64 bytes (fallback to the global function) with a call in the other arguments
--FILE--
<?php
// (a namespace long enough that "ns\function" is over 64 bytes)
namespace Symfony\Component\Messenger\Transport\Serialization\Normalizerzz;
class M { public function __construct(array $c = []) {} }
function id($x) { return $x; }
$n = [1, 2]; echo array_unshift($n, id(5)), " ", count($n), "\n";
$n = [1, 2]; echo array_unshift($n, new M()), " ", count($n), "\n";
$n = [1, 2]; echo array_push($n, id(3)), " ", \json_encode($n), "\n";
$n = [3, 1, 2]; echo \json_encode([sort($n, id(0)), $n]), "\n";
$n = [3, 1, 2]; echo \json_encode([rsort($n, id(0)), $n]), "\n";
$n = [1, 2, 3]; echo \json_encode([array_splice($n, id(0), 1), $n]), "\n";
$n = [1, 2, 3]; echo \json_encode([array_pop($n), $n]), "\n";
$n = [1, 2, 3]; echo \json_encode([end($n), current($n)]), "\n";
$n = ['b' => 1, 'a' => 2]; echo \json_encode([ksort($n, id(0)), $n]), "\n";
$n = [3, 1, 2]; echo \json_encode([usort($n, fn($a, $b) => $a <=> $b), $n]), "\n";
$s = 'a'; echo preg_match('/a/', $s, $m, id(0)), " ", \json_encode($m), "\n";

?>
--EXPECTF--
3 3
3 3
3 [1,2,3]
[true,[1,2,3]]
[true,[3,2,1]]
[[1],[2,3]]
[3,[1,2]]
[3,3]
[true,{"a":2,"b":1}]
[true,[1,2,3]]
1 ["a"]
