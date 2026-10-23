--TEST--
Resources an extension creates and hands out are freed with their last zval
--EXTENSIONS--
zip
sqlite3
--FILE--
<?php
function n() { return count(get_resources()); }
$f = __DIR__ . "/resources_from_internal_calls_freed.zip";
@unlink($f); // (creates the default stream context)
$start = n();
$z = new ZipArchive;
$z->open($f, ZipArchive::CREATE);
$z->addFromString("a", "hello");
$z->close();
$z->open($f);
$s = $z->getStream("a");
var_dump(n() - $start);
unset($s);
var_dump(n() - $start);
$z->close();

$db = new SQLite3(":memory:");
$db->exec("create table t(b blob)");
$db->exec("insert into t values(x'4142')");
$s = $db->openBlob("t", "b", 1);
var_dump(n() - $start);
unset($s);
var_dump(n() - $start);

$p = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, 0);
var_dump(n() - $start);
unset($p);
var_dump(n() - $start);

// dir(): the handle stays the default directory until the next opendir()
$d = dir(__DIR__);
unset($d);
var_dump(n() - $start);
$h = opendir(__DIR__);
var_dump(n() - $start);
closedir($h);
var_dump(n() - $start);
?>
--CLEAN--
<?php @unlink(__DIR__ . "/resources_from_internal_calls_freed.zip"); ?>
--EXPECT--
int(1)
int(0)
int(1)
int(0)
int(2)
int(0)
int(1)
int(1)
int(1)
