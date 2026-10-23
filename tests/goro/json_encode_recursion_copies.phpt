--TEST--
json_encode() detects recursion through copies of the array being encoded
--FILE--
<?php
class J implements JsonSerializable { public $d; function jsonSerialize(): mixed { return $this->d; } }
$j = new J; $j->d = [$j];
var_dump(json_encode($j, JSON_PARTIAL_OUTPUT_ON_ERROR), json_last_error());
$j2 = new J; $j2->d = $j2;
var_dump(json_encode($j2, JSON_PARTIAL_OUTPUT_ON_ERROR), json_last_error());
$o = new stdClass; $o->a = [$o];
var_dump(json_encode($o, JSON_PARTIAL_OUTPUT_ON_ERROR));
$arr = [1]; $o2 = new stdClass; $o2->r = &$arr; $arr[] = $o2;
var_dump(json_encode($arr, JSON_PARTIAL_OUTPUT_ON_ERROR));
var_dump(json_encode($o2, JSON_PARTIAL_OUTPUT_ON_ERROR));
$g = ['a' => 1]; $g['g'] = &$g; $h = $g;
var_dump(json_encode($h, JSON_PARTIAL_OUTPUT_ON_ERROR));
var_dump(json_encode($g['g'], JSON_PARTIAL_OUTPUT_ON_ERROR));
$p = [1]; $q = [&$p]; $p[] = &$q;
var_dump(json_encode($p, JSON_PARTIAL_OUTPUT_ON_ERROR));
var_dump(json_encode($q, JSON_PARTIAL_OUTPUT_ON_ERROR));
$ao = new ArrayObject([1]); $ao[] = $ao;
var_dump(json_encode($ao, JSON_PARTIAL_OUTPUT_ON_ERROR));
class K { public $self; public $arr = []; } $k = new K; $k->self = $k; $k->arr[] = $k;
var_dump(json_encode($k, JSON_PARTIAL_OUTPUT_ON_ERROR));
var_dump(json_encode([$k], JSON_PARTIAL_OUTPUT_ON_ERROR));
--EXPECTF--
string(6) "[null]"
int(6)
string(10) "{"d":null}"
int(6)
string(12) "{"a":[null]}"
string(14) "[1,{"r":null}]"
string(14) "{"r":[1,null]}"
string(16) "{"a":1,"g":null}"
string(16) "{"a":1,"g":null}"
string(10) "[1,[null]]"
string(10) "[[1,null]]"
string(16) "{"0":1,"1":null}"
string(26) "{"self":null,"arr":[null]}"
string(28) "[{"self":null,"arr":[null]}]"
