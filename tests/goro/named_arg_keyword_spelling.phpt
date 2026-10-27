--TEST--
A named argument spelled as a reserved keyword keeps its case (readOnly: names $readOnly)
--FILE--
<?php
function f($readOnly = 0, $List = 0, $Class = 0, $FN = 0, $Match = 0, $Enum = 0, $Static = 0, $Array = 0) { return func_get_args(); }
echo json_encode(f(readOnly: 1, List: 2, Class: 3, FN: 4, Match: 5, Enum: 6, Static: 7, Array: 8)), "\n";
class C { const ReadOnly = 1; const LIST = 2; public function Fn() { return 'm'; } public static function Match() { return 's'; } }
echo C::ReadOnly, C::LIST, (new C)->Fn(), C::Match(), "\n";
$o = new stdClass; $o->ReadOnly = 1; $o->Class = 2; echo json_encode($o), "\n";
enum E: string { case ReadOnly = 'r'; case Default = 'd'; }
echo E::ReadOnly->name, E::Default->name, "\n";
#[Attribute] class At { public function __construct(public $readOnly = 0, public $Enum = 0) {} }
#[At(readOnly: 1, Enum: 2)] class Q {}
var_dump((new ReflectionClass('Q'))->getAttributes()[0]->getArguments());
eval("echo json_encode(f(readOnly: 9, Match: 8)), \"\\n\";");

?>
--EXPECTF--
[1,2,3,4,5,6,7,8]
12ms
{"ReadOnly":1,"Class":2}
ReadOnlyDefault
array(2) {
  ["readOnly"]=>
  int(1)
  ["Enum"]=>
  int(2)
}
[9,0,0,0,8]
