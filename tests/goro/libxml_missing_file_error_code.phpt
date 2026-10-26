--TEST--
A missing file is XML_IO_ENOENT (1524) for DOM load(), loadHTMLFile() and SimpleXML
--FILE--
<?php
libxml_use_internal_errors(true);
$f = __DIR__ . '/nope-missing.xml';
function dump() { foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} D{$e->line}:{$e->column} ", str_replace(__DIR__, 'DIR', trim($e->message)), " file=", var_export($e->file, true), "\n"; libxml_clear_errors(); }
$d = new DOMDocument; var_dump($d->load($f)); dump();
$d = new DOMDocument; var_dump($d->loadHTMLFile($f)); dump();
var_dump(simplexml_load_file($f)); dump();
try { new SimpleXMLElement($f, 0, true); } catch (Exception $e) { echo $e->getMessage(), "\n"; } dump();
$d = new DOMDocument; var_dump($d->load($f));
var_dump(simplexml_load_file($f));
?>
--EXPECT--
bool(false)
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
bool(false)
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
bool(false)
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
String could not be parsed as XML
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
bool(false)
bool(false)
