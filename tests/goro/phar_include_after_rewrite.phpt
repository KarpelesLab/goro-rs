--TEST--
Including from a phar again after the archive file was rewritten sees the new contents
--EXTENSIONS--
phar
--INI--
phar.readonly=0
--FILE--
<?php
$f = __DIR__ . '/phar_include_after_rewrite.phar';
@unlink($f);
$p = new Phar($f);
$p['a.php'] = '<?php return "first";';
$p['b.php'] = '<?php return "b1";';
unset($p);
var_dump(include "phar://$f/a.php");
var_dump(include "phar://$f/b.php");
var_dump(include "phar://$f/a.php");
// rewritten through the extension
$p = new Phar($f);
$p['a.php'] = '<?php return "second, longer";';
unset($p);
var_dump(include "phar://$f/a.php");
var_dump(include "phar://$f/b.php");
// same contents length, rewritten behind phar's back
$p = new Phar($f);
$p['a.php'] = '<?php return "third, longer!";';
unset($p);
var_dump(include "phar://$f/a.php");
?>
--CLEAN--
<?php
@unlink(__DIR__ . '/phar_include_after_rewrite.phar');
?>
--EXPECT--
string(5) "first"
string(2) "b1"
string(5) "first"
string(14) "second, longer"
string(2) "b1"
string(14) "third, longer!"
