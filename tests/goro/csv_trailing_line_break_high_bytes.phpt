--TEST--
str_getcsv()/fgetcsv(): a line break followed only by bytes >= 0x80 is trailing
--FILE--
<?php
foreach (["a\r\x98", "a\n\x80", "a\r\n\x80\x81", "a\x80\r\n", "a,b\r\x98", "\"a\r\x98", "\"a\"\r\x98", "x\x80y\n", "a\r\n\r\n", "\r\x80", "\x80\r", "a,\"b\nc\"\r\x99", "a\n\x80\x80\x80"] as $s) {
  echo json_encode(array_map('bin2hex', str_getcsv($s, ",", '"', ""))), "\n";
}
$f = tempnam(sys_get_temp_dir(), "csv");
file_put_contents($f, "a\r\x98\nb,\"c\x80\r\nd\"\x81\ne\n");
$h = fopen($f, "r");
while (($r = fgetcsv($h, null, ",", '"', "")) !== false) {
  echo json_encode(array_map(fn($x) => $x === null ? null : bin2hex($x), $r)), "\n";
}
fclose($h);
$o = new SplFileObject($f);
$o->setFlags(SplFileObject::READ_CSV);
$o->setCsvControl(",", '"', "");
foreach ($o as $r) { echo json_encode(array_map(fn($x) => $x === null ? null : bin2hex($x), $r)), "\n"; }
unlink($f);
--EXPECT--
["61"]
["61"]
["61"]
["6180"]
["61","62"]
["610d98"]
["610d"]
["788079"]
["61"]
[""]
["80"]
["61","620a630d"]
["610a80"]
["61"]
["62","63800d0a6481"]
["65"]
["61"]
["62","63800d0a6481"]
["65"]
[null]
