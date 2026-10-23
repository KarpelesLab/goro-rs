--TEST--
RegexIterator's state is not visible as properties
--FILE--
<?php
$r = new RegexIterator(new ArrayIterator(['a1', 'b', 'c2']), '/\d/');
var_dump($r);
print_r($r); echo "\n";
var_dump((array) $r, get_object_vars($r), json_encode($r), serialize($r), isset($r->__regex_pattern));
foreach ($r as $k => $v) echo "$k=$v\n";
var_dump($r->getRegex(), $r->getMode(), $r->getFlags(), $r->getPregFlags());
$r->setMode(RegexIterator::GET_MATCH); $r->setFlags(RegexIterator::USE_KEY); $r->setPregFlags(PREG_OFFSET_CAPTURE);
var_dump($r->getMode(), $r->getFlags(), $r->getPregFlags());
class MyR extends RecursiveRegexIterator { public $mine = 1; }
print_r(new MyR(new RecursiveArrayIterator([]), "/x/"));
--EXPECT--
object(RegexIterator)#1 (1) {
  ["replacement"]=>
  NULL
}
RegexIterator Object
(
    [replacement] => 
)

array(1) {
  ["replacement"]=>
  NULL
}
array(1) {
  ["replacement"]=>
  NULL
}
string(20) "{"replacement":null}"
string(46) "O:13:"RegexIterator":1:{s:11:"replacement";N;}"
bool(false)
0=a1
2=c2
string(4) "/\d/"
int(0)
int(0)
int(0)
int(1)
int(1)
int(256)
MyR Object
(
    [replacement] => 
    [mine] => 1
)
