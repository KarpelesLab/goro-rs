--TEST--
Fast comparisons of scalars and isset() of string offsets
--FILE--
<?php
function c($a, $b) { return [$a < $b, $a <= $b, $a > $b, $a >= $b]; }
$vals = [null, true, false, 0, 1, -1, 1.5, NAN, INF, "", "0", "1", "a", "A", "abc", "ab", " 1", "1e3", "1000", "10", "9", "0x1", "-", ".", "1a", "é"];
$out = '';
foreach ($vals as $a) foreach ($vals as $b) { foreach (c($a, $b) as $r) $out .= $r ? '1' : '0'; $out .= ' '; }
echo md5($out), "\n", substr($out, 0, 400), "\n";
$s = "hello";
function iss($s, $i) { return [isset($s[$i]), $s[$i] ?? 'none']; }
foreach ([0, 4, 5, -1, -5, -6, 100] as $i) echo json_encode(iss($s, $i));
echo "\n";
class L { public $sql = "abc"; public $p = 0; function n() { $b = $this->sql[$this->p] ?? null; $this->p++; return $b; } }
$l = new L; echo json_encode([$l->n(), $l->n(), $l->n(), $l->n()]), "\n";
--EXPECT--
a58bd2be8e81b71c124c41117c6efce1
0101 1100 0101 0101 1100 1100 1100 1100 1100 0101 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 0011 0101 0011 0011 0101 0101 0101 0101 0101 0011 0011 0101 0101 0101 0101 0101 0101 0101 0101 0101 0101 0101 0101 0101 0101 0101 0101 1100 0101 0101 1100 1100 1100 1100 1100 0101 0101 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 1100 0101 1100 
[true,"h"][true,"o"][false,"none"][true,"o"][true,"h"][false,"none"][false,"none"]
["a","b","c",null]
