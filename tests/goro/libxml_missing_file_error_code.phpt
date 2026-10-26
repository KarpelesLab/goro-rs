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
$d = new DOMDocument; $d->loadXML('<!DOCTYPE a SYSTEM "nope.dtd"><a/>', LIBXML_DTDLOAD); dump();
$d = new DOMDocument; $d->loadXML('<!DOCTYPE a [<!ENTITY e SYSTEM "nope.ent">]><a>&e;</a>', LIBXML_NOENT); dump();
libxml_use_internal_errors(false);
$d = new DOMDocument; var_dump($d->load($f));
var_dump(simplexml_load_file($f));
?>
--EXPECTF--
bool(false)
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
bool(false)
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
bool(false)
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
String could not be parsed as XML
  L1 C1524 D0:0 failed to load "DIR/nope-missing.xml": No such file or directory file=''
  L1 C1524 D1:31 failed to load "DIR/nope.dtd": No such file or directory file=''
  L1 C1524 D1:51 failed to load "DIR/nope.ent": No such file or directory file=''

Warning: DOMDocument::load(): I/O warning : failed to load "/tmp/claude-1000/-home-magicaltux-projects-goro-rs/3d2a04d8-60d8-47cf-8c66-cf536b5391ca/scratchpad/ext1/xml/nope-missing.xml": No such file or directory in %s on line 12
bool(false)

Warning: simplexml_load_file(): I/O warning : failed to load "/tmp/claude-1000/-home-magicaltux-projects-goro-rs/3d2a04d8-60d8-47cf-8c66-cf536b5391ca/scratchpad/ext1/xml/nope-missing.xml": No such file or directory in %s on line 13
bool(false)
