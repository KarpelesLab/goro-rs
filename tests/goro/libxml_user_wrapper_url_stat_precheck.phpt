--TEST--
libxml loads through stream wrappers: url_stat precheck of user wrappers, SplFileObject warnings on user streams, gzip sniffing
--FILE--
<?php
class MemW {
  static $files = [];
  public $context; private $p; private $pos = 0; private $mode;
  function stream_open($path, $mode, $opts, &$opened) { echo "open $path $mode\n"; $this->p = $path; $this->mode = $mode; if ($mode[0] == 'w') self::$files[$path] = ''; return isset(self::$files[$path]); }
  function stream_read($n) { $r = substr(self::$files[$this->p], $this->pos, $n); $this->pos += strlen($r); return $r; }
  function stream_write($d) { self::$files[$this->p] .= $d; return strlen($d); }
  function stream_eof() { return $this->pos >= strlen(self::$files[$this->p]); }
  function stream_stat() { return ['size' => strlen(self::$files[$this->p])]; }
  function url_stat($p, $f) { return isset(self::$files[$p]) ? ['size' => strlen(self::$files[$p]), 'mode' => 0100644] : false; }
  function stream_close() { echo "close {$this->p}\n"; }
  function stream_flush() { return true; }
}
stream_wrapper_register('mem', 'MemW');
MemW::$files['mem://a.xml'] = '<r><a x="1">t</a><b/></r>';
$d = new DOMDocument; var_dump($d->load('mem://a.xml')); echo $d->saveXML();
var_dump(simplexml_load_file('mem://a.xml')->a['x'] . "");
var_dump($d->save('mem://out.xml'), MemW::$files['mem://out.xml']);
$w = new XMLWriter; $w->openUri('mem://w.xml'); $w->writeElement('x', 'y'); $w->flush(); unset($w); var_dump(MemW::$files['mem://w.xml']);
var_dump(@$d->load('mem://missing.xml'));
try { $f = new SplFileObject("mem://a.xml"); foreach ($f as $l) var_dump($l); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } unset($f);
$p = xml_parser_create(); var_dump(xml_parse($p, file_get_contents('mem://a.xml'), true));
$r = XMLReader::open('data://text/plain;base64,' . base64_encode('<z>1</z>')); $r->read(); var_dump($r->name, $r->readString());
file_put_contents('compress.zlib://' . sys_get_temp_dir() . '/goro_libxml_gz.xml.gz', '<gz>zipped</gz>');
$d2 = new DOMDocument; var_dump($d2->load('compress.zlib://' . sys_get_temp_dir() . '/goro_libxml_gz.xml.gz'), $d2->documentElement->textContent);
$d3 = new DOMDocument; var_dump($d3->load(sys_get_temp_dir() . '/goro_libxml_gz.xml.gz'), $d3->documentElement?->textContent);
var_dump(simplexml_load_file(sys_get_temp_dir() . '/goro_libxml_gz.xml.gz') . "");
$r = XMLReader::open(sys_get_temp_dir() . '/goro_libxml_gz.xml.gz'); $r->read(); var_dump($r->name);
libxml_set_streams_context(stream_context_create(['mem' => ['x' => 1]]));
$d4 = new DOMDocument; $d4->load('mem://a.xml'); var_dump($d4->documentElement->nodeName);
?>
--EXPECTF--
open mem://a.xml rb
close mem://a.xml
bool(true)
<?xml version="1.0"?>
<r><a x="1">t</a><b/></r>
open mem://a.xml rb
close mem://a.xml
string(1) "1"
open mem://out.xml wb
close mem://out.xml
int(48)
string(48) "<?xml version="1.0"?>
<r><a x="1">t</a><b/></r>
"
open mem://w.xml wb
close mem://w.xml
string(8) "<x>y</x>"
bool(false)
open mem://a.xml r

Warning: SplFileObject::rewind(): Stream does not support seeking in %s on line 21
RuntimeException: Cannot rewind file mem://a.xml
close mem://a.xml
open mem://a.xml rb
close mem://a.xml
int(1)
string(1) "z"
string(1) "1"
bool(true)
string(6) "zipped"

Warning: DOMDocument::load(): Start tag expected, '<' not found in /tmp/goro_libxml_gz.xml.gz, line: 1 in %s on line 26
bool(false)
NULL

Warning: simplexml_load_file(): /tmp/goro_libxml_gz.xml.gz:1: parser error : Start tag expected, '<' not found in %s on line 27

Warning: simplexml_load_file():  in %s on line 27

Warning: simplexml_load_file(): ^ in %s on line 27
string(0) ""

Warning: XMLReader::read(): /tmp/goro_libxml_gz.xml.gz:1: parser error : Start tag expected, '<' not found in %s on line 28

Warning: XMLReader::read():  in %s on line 28

Warning: XMLReader::read(): ^ in %s on line 28
string(0) ""
open mem://a.xml rb
close mem://a.xml
string(1) "r"
