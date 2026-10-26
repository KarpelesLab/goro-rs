--TEST--
Anonymous class name suffixes run on across includes and eval()
--FILE--
<?php
function n($o) { return strstr(get_class($o), '$'); }
echo n(new class {}), "\n";
echo n(eval('return new class {};')), "\n";
file_put_contents(__DIR__ . "/anon_ctr2.inc", '<?php return new class {};');
echo n(include __DIR__ . "/anon_ctr2.inc"), "\n";
echo n(new class {}), "\n";
unlink(__DIR__ . "/anon_ctr2.inc");
--EXPECT--
$0
$2
$3
$1
