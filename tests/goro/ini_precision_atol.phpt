--TEST--
precision / serialize_precision INI values: ZEND_ATOL() of the string, refused below -1
--FILE--
<?php
foreach (['abc', '100', '-5', ' 7', '7x', '3.9', '', '-2', '0x10', '+4'] as $v) {
  ini_set('precision', '14'); ini_set('serialize_precision', '-1');
  echo json_encode($v), ": ";
  var_dump(ini_set('precision', $v), ini_get('precision'));
  echo 1/3, " ", 1e15/7, "\n";
  var_dump(ini_set('serialize_precision', $v), ini_get('serialize_precision'));
  var_dump(1/3); echo json_encode(1/3), " ", serialize(1/3), " ", var_export(1/3, true), "\n";
}
ini_set('precision', '14'); ini_set('serialize_precision', '-1');
var_dump(ini_set('precision', 17.9)); echo 0.1, "\n";
var_dump(ini_set('precision', true)); echo 0.1, "\n";
var_dump(ini_set('precision', null)); echo 0.1, "\n";
--EXPECTF--
"abc": string(2) "14"
string(3) "abc"
0.3 1.0E+14
string(2) "-1"
string(3) "abc"
float(0.3)
0.3 d:0.3; 0.3
"100": string(2) "14"
string(3) "100"
0.333333333333333314829616256247390992939472198486328125 142857142857142.84375
string(2) "-1"
string(3) "100"
float(0.333333333333333314829616256247390992939472198486328125)
0.333333333333333314829616256247390992939472198486328125 d:0.333333333333333314829616256247390992939472198486328125; 0.333333333333333314829616256247390992939472198486328125
"-5": bool(false)
string(2) "14"
0.33333333333333 1.4285714285714E+14
bool(false)
string(2) "-1"
float(0.3333333333333333)
0.3333333333333333 d:0.3333333333333333; 0.3333333333333333
" 7": string(2) "14"
string(2) " 7"
0.3333333 1.428571E+14
string(2) "-1"
string(2) " 7"
float(0.3333333)
0.3333333 d:0.3333333; 0.3333333
"7x": string(2) "14"
string(2) "7x"
0.3333333 1.428571E+14
string(2) "-1"
string(2) "7x"
float(0.3333333)
0.3333333 d:0.3333333; 0.3333333
"3.9": string(2) "14"
string(3) "3.9"
0.333 1.43E+14
string(2) "-1"
string(3) "3.9"
float(0.333)
0.333 d:0.333; 0.333
"": string(2) "14"
string(0) ""
0.3 1.0E+14
string(2) "-1"
string(0) ""
float(0.3)
0.3 d:0.3; 0.3
"-2": bool(false)
string(2) "14"
0.33333333333333 1.4285714285714E+14
bool(false)
string(2) "-1"
float(0.3333333333333333)
0.3333333333333333 d:0.3333333333333333; 0.3333333333333333
"0x10": string(2) "14"
string(4) "0x10"
0.3 1.0E+14
string(2) "-1"
string(4) "0x10"
float(0.3)
0.3 d:0.3; 0.3
"+4": string(2) "14"
string(2) "+4"
0.3333 1.429E+14
string(2) "-1"
string(2) "+4"
float(0.3333)
0.3333 d:0.3333; 0.3333
string(2) "14"
0.10000000000000001
string(4) "17.9"
0.1
string(1) "1"
0.1
