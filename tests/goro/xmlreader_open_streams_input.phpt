--TEST--
XMLReader::open() pulls its input from the stream (user wrappers, files) as read() advances: endless inputs, late errors, stream_read() exceptions, stream diagnostics named after the method
--FILE--
<?php
class W {
    public $context;
    static $n = 0;
    static $per = 1500;
    static $c = 0;
    private $mode;
    function stream_open($path, $mode, $options, &$opened) {
        echo "open($path, $mode)\n";
        $this->mode = parse_url($path, PHP_URL_HOST);
        return true;
    }
    function stream_read($count) {
        $i = self::$n++;
        echo "read($count) #$i at node ", self::$c, "\n";
        if ($this->mode === 'endless') {
            if ($i == 0) return '<r>' . str_repeat('<a/>', 1000);
            return str_repeat('<a/>', self::$per);
        }
        if ($this->mode === 'late') {
            if ($i < 3) return ($i == 0 ? '<r>' : '') . str_repeat('<b>x</b>', 1000);
            if ($i == 3) return '<bad attr></r>';
            return '';
        }
        if ($this->mode === 'throw') {
            if ($i == 2) throw new Exception("read failed");
            return ($i == 0 ? '<r>' : '') . str_repeat('<a/>', 1000);
        }
        if ($this->mode === 'big') {
            if ($i == 0) return '<r>';
            if ($i < 3) return str_repeat('<a/>', 2100);
            return '</r>';
        }
        if ($i == 0) return '<r><a>1</a>';
        if ($i == 1) return '<a>2</a></r>';
        return '';
    }
    function stream_eof() { return $this->mode === 'small' && self::$n > 2; }
    function url_stat($p, $f) { echo "url_stat($p, $f)\n"; return []; }
    function stream_close() { echo "close\n"; }
}
stream_wrapper_register('wrap', 'W');

echo "--- small\n";
$r = new XMLReader;
var_dump($r->open('wrap://small/x.xml'));
echo "opened\n";
while ($r->read()) { W::$c++; echo "node ", $r->nodeType, " ", $r->name, "\n"; }
$r->close();
echo "closed\n";

foreach ([1500, 100, 10] as $per) {
    echo "--- endless $per\n";
    W::$n = 0; W::$c = 0; W::$per = $per;
    $r = XMLReader::open('wrap://endless/x.xml');
    echo "opened\n";
    while ($r->read()) { if (++W::$c > ($per < 100 ? 1100 : 3000)) break; }
    echo "stopped at ", W::$c, "\n";
    unset($r);
    echo "unset\n";
}

echo "--- late error\n";
W::$n = 0; W::$c = 0;
$r = XMLReader::open('wrap://late/x.xml');
while ($r->read()) { W::$c++; }
echo "stopped at ", W::$c, "\n";
unset($r);

echo "--- stream_read throws\n";
W::$n = 0; W::$c = 0;
$r = XMLReader::open('wrap://throw/x.xml');
try { while ($r->read()) W::$c++; echo "end\n"; } catch (Exception $e) { echo "caught ", $e->getMessage(), " at ", W::$c, "\n"; }
var_dump($r->read());
unset($r);

echo "--- stream_read returns too much\n";
W::$n = 0; W::$c = -1000000;
$r = XMLReader::open('wrap://big/x.xml');
$c = 0;
while ($r->read()) $c++;
echo "$c\n";
unset($r);

class NoStat {
    public $context;
    function stream_open($path, $mode, $options, &$opened) { echo "open\n"; return true; }
}
stream_wrapper_register('nostat', 'NoStat');
var_dump(XMLReader::open('nostat://x'));
class Fail {
    public $context;
    function stream_open($path, $mode, $options, &$opened) { echo "open fails\n"; return false; }
    function url_stat($p, $f) { echo "url_stat $f\n"; return []; }
}
stream_wrapper_register('fail', 'Fail');
var_dump(XMLReader::open('fail://x'));
try { XMLReader::fromUri('fail://x'); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(XMLReader::open('php://nope'));

echo "--- local file\n";
$f = tempnam(sys_get_temp_dir(), 'xr');
file_put_contents($f, '<r>' . str_repeat('<a/>', 2000));
$r = XMLReader::open($f);
for ($i = 0; $i < 10; $i++) $r->read();
file_put_contents($f, str_repeat('<b/>', 10) . '</r>', FILE_APPEND);
$c = 10;
while ($r->read()) { $c++; $last = $r->name; }
echo "grown $c $last\n";
unlink($f);
$r = XMLReader::open('data:text/xml,<r><x/></r>');
while ($r->read()) echo $r->name, "\n";
$r = XMLReader::XML('<s><t/></s>');
while ($r->read()) echo $r->name, "\n";
echo "done\n";
--EXPECTF--
--- small
url_stat(wrap://small/x.xml, 2)
open(wrap://small/x.xml, rb)
read(8192) #0 at node 0
bool(true)
opened
read(8192) #1 at node 0
read(8192) #2 at node 0
node 1 r
node 1 a
node 3 #text
node 15 a
node 1 a
node 3 #text
node 15 a
node 15 r
close
closed
--- endless 1500
url_stat(wrap://endless/x.xml, 2)
open(wrap://endless/x.xml, rb)
read(8192) #0 at node 0
opened
read(8192) #1 at node 897
read(8192) #2 at node 1921
read(8192) #3 at node 2945
stopped at 3001
close
unset
--- endless 100
url_stat(wrap://endless/x.xml, 2)
open(wrap://endless/x.xml, rb)
read(8192) #0 at node 0
opened
read(8192) #1 at node 897
read(8192) #2 at node 1025
read(8192) #3 at node 1153
read(8192) #4 at node 1281
read(8192) #5 at node 1401
read(8192) #6 at node 1501
read(8192) #7 at node 1601
read(8192) #8 at node 1701
read(8192) #9 at node 1801
read(8192) #10 at node 1901
read(8192) #11 at node 2001
read(8192) #12 at node 2101
read(8192) #13 at node 2201
read(8192) #14 at node 2301
read(8192) #15 at node 2401
read(8192) #16 at node 2501
read(8192) #17 at node 2601
read(8192) #18 at node 2701
read(8192) #19 at node 2801
read(8192) #20 at node 2901
stopped at 3001
close
unset
--- endless 10
url_stat(wrap://endless/x.xml, 2)
open(wrap://endless/x.xml, rb)
read(8192) #0 at node 0
opened
read(8192) #1 at node 897
read(8192) #2 at node 1011
read(8192) #3 at node 1021
read(8192) #4 at node 1031
read(8192) #5 at node 1041
read(8192) #6 at node 1051
read(8192) #7 at node 1061
read(8192) #8 at node 1071
read(8192) #9 at node 1081
read(8192) #10 at node 1091
stopped at 1101
close
unset
--- late error
url_stat(wrap://late/x.xml, 2)
open(wrap://late/x.xml, rb)
read(8192) #0 at node 0
read(8192) #1 at node 1343
read(8192) #2 at node 4415
read(8192) #3 at node 7487
read(8192) #4 at node 8831

Warning: XMLReader::read(): wrap://late/x.xml:1: parser error : Specification mandates value for attribute attr in %s on line 66

Warning: XMLReader::read(): b>x</b><b>x</b><b>x</b><b>x</b><b>x</b><b>x</b><b>x</b><b>x</b><b>x</b><bad attr in %s on line 66

Warning: XMLReader::read():                                                                                ^ in %s on line 66
stopped at 8831
close
--- stream_read throws
url_stat(wrap://throw/x.xml, 2)
open(wrap://throw/x.xml, rb)
read(8192) #0 at node 0
read(8192) #1 at node 897
read(8192) #2 at node 1921
caught read failed at 1921
bool(false)
close
--- stream_read returns too much
url_stat(wrap://big/x.xml, 2)
open(wrap://big/x.xml, rb)
read(8192) #0 at node -1000000
read(8192) #1 at node -1000000

Warning: XMLReader::read(): W::stream_read - read 208 bytes more data than requested (8400 read, 8192 max) - excess data will be lost in %s on line 81
read(8192) #2 at node -1000000

Warning: XMLReader::read(): W::stream_read - read 208 bytes more data than requested (8400 read, 8192 max) - excess data will be lost in %s on line 81
read(8192) #3 at node -1000000
read(8192) #4 at node -1000000

Warning: XMLReader::read(): wrap://big/x.xml:1: parser error : Extra content at the end of the document in %s on line 81

Warning: XMLReader::read(): <a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/><a/></r> in %s on line 81

Warning: XMLReader::read():                                                                                ^ in %s on line 81
4097
close

Warning: XMLReader::open(): NoStat::url_stat is not implemented! in %s on line 90

Warning: XMLReader::open(): Unable to open source data in %s on line 90
bool(false)
url_stat 2
open fails

Warning: XMLReader::open(fail://x): Failed to open stream: "Fail::stream_open" call failed in %s on line 97

Warning: XMLReader::open(): Unable to open source data in %s on line 97
bool(false)
url_stat 2
open fails

Warning: XMLReader::fromUri(fail://x): Failed to open stream: "Fail::stream_open" call failed in %s on line 98
Error: Unable to open source data

Warning: XMLReader::open(): Invalid php:// URL specified in %s on line 99

Warning: XMLReader::open(php://nope): Failed to open stream: operation failed in %s on line 99

Warning: XMLReader::open(): Unable to open source data in %s on line 99
bool(false)
--- local file
grown 2012 r
r
x
r
s
t
s
done
