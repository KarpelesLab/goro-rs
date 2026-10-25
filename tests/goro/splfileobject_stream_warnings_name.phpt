--TEST--
SplFileObject: notices of the stream functions a method runs name the method
--FILE--
<?php
$o = new SplFileObject(__FILE__);
var_dump($o->fwrite("x"));
$w = fopen(__FILE__, 'r');
var_dump(fputs($w, "y"));
var_dump(fwrite($w, "z"));
?>
--EXPECTF--
Notice: SplFileObject::fwrite(): Write of 1 bytes failed with errno=9 Bad file descriptor in %s on line %d
bool(false)

Notice: fputs(): Write of 1 bytes failed with errno=9 Bad file descriptor in %s on line %d
bool(false)

Notice: fwrite(): Write of 1 bytes failed with errno=9 Bad file descriptor in %s on line %d
bool(false)
