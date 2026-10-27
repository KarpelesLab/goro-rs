--TEST--
phar directory stream: entry names up to MAXPATHLEN-1 bytes are read
--INI--
phar.readonly=0
--FILE--
<?php
var_dump(Phar::canWrite());
$p = sys_get_temp_dir() . "/goro_pharlong_" . getmypid() . ".phar";
$phar = new Phar($p);
$phar->startBuffering();
$phar->addFromString(str_repeat("A", PHP_MAXPATHLEN - 1), "c1");
$phar->addFromString(str_repeat("B", PHP_MAXPATHLEN - 1)."C", "c2");
$phar->stopBuffering();
$h = opendir("phar://" . $p);
var_dump(strlen(readdir($h)), readdir($h));
closedir($h);


unset($phar); Phar::unlinkArchive($p);
--EXPECT--
bool(true)
int(4095)
bool(false)
