--TEST--
stat functions, SplFileInfo and file_get_contents() on file:// URLs
--FILE--
<?php
$p = 'file://' . __FILE__;
var_dump(is_file($p), file_exists($p), is_readable($p), filesize($p) === filesize(__FILE__), is_dir('file://' . __DIR__));
var_dump(is_file('file://localhost' . __FILE__), is_file('file:///' . __FILE__));
$f = new SplFileInfo($p);
var_dump($f->isFile(), $f->getRealPath(), $f->getSize() === filesize(__FILE__), $f->getPathname() === $p);
var_dump(realpath($p), filemtime($p) === filemtime(__FILE__), is_link($p), fileperms($p) === fileperms(__FILE__));
var_dump(file_get_contents($p) === file_get_contents(__FILE__));
var_dump(stat($p)['size'] === filesize(__FILE__));
var_dump(@filesize('file:///nonexistent/goro-x'), file_exists('file:///nonexistent/goro-x'));

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
bool(false)
