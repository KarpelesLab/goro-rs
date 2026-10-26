--TEST--
Reflection of internal class constants registered by extensions (XMLReader, DOMNode, Dom\Node)
--FILE--
<?php
var_dump((new ReflectionClass('XMLReader'))->getConstant('ELEMENT'));
var_dump((new ReflectionClassConstant('DOMNode', 'DOCUMENT_POSITION_PRECEDING'))->getValue());
var_dump((new ReflectionClassConstant('Dom\Node', 'DOCUMENT_POSITION_PRECEDING'))->getValue());
var_dump(count((new ReflectionClass('XMLReader'))->getConstants()));
var_dump((new ReflectionClass('XMLReader'))->getConstants()['ATTRIBUTE']);
echo new ReflectionClassConstant('XMLReader', 'ELEMENT');
echo new ReflectionClassConstant('DOMNode', 'DOCUMENT_POSITION_PRECEDING');
echo new ReflectionClassConstant('Dom\Element', 'DOCUMENT_POSITION_FOLLOWING');
var_dump(XMLReader::ELEMENT, DOMNode::DOCUMENT_POSITION_PRECEDING);
$s = (string) new ReflectionClass('XMLReader');
preg_match_all('/Constant \[.*\]/', $s, $m); echo implode("\n", array_slice($m[0], 0, 3)), "\n";
$s = (string) new ReflectionClass('Dom\Node');
preg_match_all('/Constant \[.*\]/', $s, $m); echo implode("\n", array_slice($m[0], 0, 3)), "\n";
foreach (['XMLReader', 'DOMNode', 'Dom\Node', 'Dom\Element', 'DOMElement'] as $c) {
  $s = (string) new ReflectionClass($c);
  preg_match_all('/Constant \[.*/', $s, $m); echo implode("\n", $m[0]), "\n";
}
--EXPECT--
int(1)
int(2)
int(2)
int(22)
int(2)
Constant [ public int ELEMENT ] { 1 }
Constant [ public int DOCUMENT_POSITION_PRECEDING ] { 2 }
Constant [ public int DOCUMENT_POSITION_FOLLOWING ] { 4 }
int(1)
int(2)
Constant [ public int NONE ]
Constant [ public int ELEMENT ]
Constant [ public int ATTRIBUTE ]
Constant [ public int DOCUMENT_POSITION_DISCONNECTED ]
Constant [ public int DOCUMENT_POSITION_PRECEDING ]
Constant [ public int DOCUMENT_POSITION_FOLLOWING ]
Constant [ public int NONE ] { 0 }
Constant [ public int ELEMENT ] { 1 }
Constant [ public int ATTRIBUTE ] { 2 }
Constant [ public int TEXT ] { 3 }
Constant [ public int CDATA ] { 4 }
Constant [ public int ENTITY_REF ] { 5 }
Constant [ public int ENTITY ] { 6 }
Constant [ public int PI ] { 7 }
Constant [ public int COMMENT ] { 8 }
Constant [ public int DOC ] { 9 }
Constant [ public int DOC_TYPE ] { 10 }
Constant [ public int DOC_FRAGMENT ] { 11 }
Constant [ public int NOTATION ] { 12 }
Constant [ public int WHITESPACE ] { 13 }
Constant [ public int SIGNIFICANT_WHITESPACE ] { 14 }
Constant [ public int END_ELEMENT ] { 15 }
Constant [ public int END_ENTITY ] { 16 }
Constant [ public int XML_DECLARATION ] { 17 }
Constant [ public int LOADDTD ] { 1 }
Constant [ public int DEFAULTATTRS ] { 2 }
Constant [ public int VALIDATE ] { 3 }
Constant [ public int SUBST_ENTITIES ] { 4 }
Constant [ public int DOCUMENT_POSITION_DISCONNECTED ] { 1 }
Constant [ public int DOCUMENT_POSITION_PRECEDING ] { 2 }
Constant [ public int DOCUMENT_POSITION_FOLLOWING ] { 4 }
Constant [ public int DOCUMENT_POSITION_CONTAINS ] { 8 }
Constant [ public int DOCUMENT_POSITION_CONTAINED_BY ] { 16 }
Constant [ public int DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC ] { 32 }
Constant [ public int DOCUMENT_POSITION_DISCONNECTED ] { 1 }
Constant [ public int DOCUMENT_POSITION_PRECEDING ] { 2 }
Constant [ public int DOCUMENT_POSITION_FOLLOWING ] { 4 }
Constant [ public int DOCUMENT_POSITION_CONTAINS ] { 8 }
Constant [ public int DOCUMENT_POSITION_CONTAINED_BY ] { 16 }
Constant [ public int DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC ] { 32 }
Constant [ public int DOCUMENT_POSITION_DISCONNECTED ] { 1 }
Constant [ public int DOCUMENT_POSITION_PRECEDING ] { 2 }
Constant [ public int DOCUMENT_POSITION_FOLLOWING ] { 4 }
Constant [ public int DOCUMENT_POSITION_CONTAINS ] { 8 }
Constant [ public int DOCUMENT_POSITION_CONTAINED_BY ] { 16 }
Constant [ public int DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC ] { 32 }
Constant [ public int DOCUMENT_POSITION_DISCONNECTED ] { 1 }
Constant [ public int DOCUMENT_POSITION_PRECEDING ] { 2 }
Constant [ public int DOCUMENT_POSITION_FOLLOWING ] { 4 }
Constant [ public int DOCUMENT_POSITION_CONTAINS ] { 8 }
Constant [ public int DOCUMENT_POSITION_CONTAINED_BY ] { 16 }
Constant [ public int DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC ] { 32 }
