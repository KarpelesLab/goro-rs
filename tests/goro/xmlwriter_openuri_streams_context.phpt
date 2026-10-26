--TEST--
XMLWriter::openURI() opens a stream wrapper URI with the libxml streams context
--FILE--
<?php
class W {
    public $context;
    static $data = '';
    function stream_open(string $path, string $mode, int $options, ?string &$opened = null): bool {
        echo "open $path $mode\n";
        $o = stream_context_get_options($this->context);
        var_dump(isset($o['mem']['obj']) ? get_class($o['mem']['obj']) : $o);
        return true;
    }
    function stream_write($d) { self::$data .= $d; return strlen($d); }
    function stream_flush() { return true; }
}
stream_wrapper_register('mem', 'W');
libxml_set_streams_context(stream_context_create(['mem' => ['obj' => new ArrayObject([])]]));
$w = new XMLWriter();
var_dump($w->openURI('mem://stream'));
var_dump($w->startDocument());
var_dump($w->writeElement('foo'));
var_dump($w->endDocument());
var_dump($w->flush());
var_dump(W::$data);

?>
--EXPECT--
open mem://stream wb
string(11) "ArrayObject"
bool(true)
bool(true)
bool(true)
bool(true)
int(0)
string(29) "<?xml version="1.0"?>
<foo/>
"
