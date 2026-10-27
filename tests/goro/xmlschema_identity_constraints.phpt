--TEST--
XML Schema identity constraints (xs:unique, xs:key, xs:keyref) are validated
--FILE--
<?php
function check(string $name, string $xsd, string $xml): void {
    echo "== $name\n";
    libxml_use_internal_errors(true);
    libxml_clear_errors();
    $d = new DOMDocument();
    $d->loadXML($xml);
    $r = @$d->schemaValidateSource($xsd);
    var_dump($r);
    foreach (libxml_get_errors() as $e) {
        echo "  [{$e->level} {$e->code}] line {$e->line}: ", trim($e->message), "\n";
    }
    libxml_clear_errors();
}

$base = <<<'XSD'
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t" targetNamespace="urn:t" elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" maxOccurs="unbounded">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="code" type="xs:integer" minOccurs="0"/>
              <xs:element name="sub" minOccurs="0" maxOccurs="unbounded">
                <xs:complexType><xs:attribute name="v" type="xs:string"/></xs:complexType>
              </xs:element>
            </xs:sequence>
            <xs:attribute name="id" type="xs:string"/>
            <xs:attribute name="num" type="xs:decimal"/>
            <xs:attribute name="def" type="xs:string" default="dflt"/>
          </xs:complexType>
        </xs:element>
        <xs:element name="ref" minOccurs="0" maxOccurs="unbounded">
          <xs:complexType><xs:attribute name="to" type="xs:string"/><xs:attribute name="n" type="xs:integer"/></xs:complexType>
        </xs:element>
      </xs:sequence>
    </xs:complexType>
    %IDC%
  </xs:element>
</xs:schema>
XSD;

function x(string $idc): string { global $base; return str_replace('%IDC%', $idc, $base); }

$doc = fn(string $body) => '<root xmlns="urn:t">' . $body . '</root>';

$unique = '<xs:unique name="U"><xs:selector xpath="t:item"/><xs:field xpath="@id"/></xs:unique>';
check('unique ok', x($unique), $doc('<item id="a"/><item id="b"/><item/>'));
check('unique dup', x($unique), $doc('<item id="a"/><item id="b"/><item id="a"/><item id="a"/>'));

$key = '<xs:key name="K"><xs:selector xpath="t:item"/><xs:field xpath="@id"/></xs:key>';
check('key missing', x($key), $doc('<item id="a"/><item/>'));
check('key dup', x($key), $doc('<item id="a"/><item id="a"/>'));

$keyref = $key . '<xs:keyref name="R" refer="t:K"><xs:selector xpath="t:ref"/><xs:field xpath="@to"/></xs:keyref>';
check('keyref ok', x($keyref), $doc('<item id="a"/><item id="b"/><ref to="b"/><ref to="a"/>'));
check('keyref missing', x($keyref), $doc("<item id=\"a\"/>\n<item id=\"b\"/>\n<ref to=\"c\"/>\n<ref to=\"a\"/><ref/>"));
check('keyref dup key', x($keyref), $doc('<item id="a"/><item id="a"/><ref to="a"/>'));

$num = '<xs:unique name="N"><xs:selector xpath="t:item"/><xs:field xpath="@num"/></xs:unique>';
check('decimal dup', x($num), $doc('<item num="1"/><item num="1.0"/><item num="01.50"/><item num="1.5"/>'));

$elemField = '<xs:unique name="E"><xs:selector xpath="t:item"/><xs:field xpath="t:code"/></xs:unique>';
check('element field dup', x($elemField), $doc('<item><code>7</code></item><item><code>+007</code></item>'));
check('element field invalid', x($elemField), $doc('<item><code>x</code></item><item><code>8</code></item>'));

$multi = '<xs:unique name="M"><xs:selector xpath="t:item"/><xs:field xpath="@id"/><xs:field xpath="t:code"/></xs:unique>';
check('multi', x($multi), $doc('<item id="a"><code>1</code></item><item id="a"><code>2</code></item><item id="a"><code>1</code></item>'));

$many = '<xs:unique name="S"><xs:selector xpath="t:item"/><xs:field xpath="t:sub/@v"/></xs:unique>';
check('field more than one', x($many), $doc('<item><sub v="1"/><sub v="2"/></item>'));

$nonSimple = '<xs:unique name="NS"><xs:selector xpath="t:item"/><xs:field xpath="t:sub"/></xs:unique>';
check('field non simple', x($nonSimple), $doc('<item><sub v="1"/></item>'));

$def = '<xs:unique name="D"><xs:selector xpath="t:item"/><xs:field xpath="@def"/></xs:unique>';
check('default attr dup', x($def), $doc('<item/><item def="x"/><item/>'));

$union = '<xs:unique name="UN"><xs:selector xpath="t:item|t:ref"/><xs:field xpath="@id|@to"/></xs:unique>';
check('union', x($union), $doc('<item id="a"/><ref to="a"/>'));

$desc = '<xs:unique name="DS"><xs:selector xpath=".//t:sub"/><xs:field xpath="@v"/></xs:unique>';
check('descendant', x($desc), $doc('<item><sub v="1"/></item><item><sub v="2"/><sub v="1"/></item>'));

$bad = '<xs:unique name="B"><xs:selector xpath="@id"/><xs:field xpath="@id"/></xs:unique>';
check('bad selector', x($bad), $doc('<item/>'));
$bad2 = '<xs:unique name="B2"><xs:selector xpath="q:item"/><xs:field xpath="@id"/></xs:unique>';
check('bad prefix', x($bad2), $doc('<item/>'));
$bad3 = '<xs:unique name="B3"><xs:selector xpath="/t:item"/><xs:field xpath="."/></xs:unique>';
check('bad root', x($bad3), $doc('<item/>'));

$keyrefNum = '<xs:key name="K2"><xs:selector xpath="t:item"/><xs:field xpath="t:code"/></xs:key><xs:keyref name="R2" refer="t:K2"><xs:selector xpath="t:ref"/><xs:field xpath="@n"/></xs:keyref>';
check('keyref numeric', x($keyrefNum), $doc('<item><code>5</code></item><ref n="05"/><ref n="6"/>'));

// nested scopes: unique per item
$nested = <<<'XSD'
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType><xs:sequence>
      <xs:element name="group" maxOccurs="unbounded">
        <xs:complexType><xs:sequence>
          <xs:element name="e" maxOccurs="unbounded"><xs:complexType><xs:attribute name="id" type="xs:string"/></xs:complexType></xs:element>
        </xs:sequence></xs:complexType>
        <xs:key name="GK"><xs:selector xpath="e"/><xs:field xpath="@id"/></xs:key>
      </xs:element>
      <xs:element name="r" maxOccurs="unbounded" minOccurs="0"><xs:complexType><xs:attribute name="to" type="xs:string"/></xs:complexType></xs:element>
    </xs:sequence></xs:complexType>
    <xs:keyref name="GR" refer="GK"><xs:selector xpath="r"/><xs:field xpath="@to"/></xs:keyref>
  </xs:element>
</xs:schema>
XSD;
check('nested ok', $nested, '<root><group><e id="a"/><e id="b"/></group><group><e id="c"/></group><r to="c"/><r to="a"/></root>');
check('nested dup in group', $nested, '<root><group><e id="a"/><e id="a"/></group><group><e id="a"/></group></root>');
check('nested across groups', $nested, '<root><group><e id="a"/></group><group><e id="a"/></group><r to="a"/><r to="z"/></root>');

?>
--EXPECT--
== unique ok
bool(true)
== unique dup
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['a'] in unique identity-constraint '{urn:t}U'.
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['a'] in unique identity-constraint '{urn:t}U'.
== key missing
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Not all fields of key identity-constraint '{urn:t}K' evaluate to a node.
== key dup
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['a'] in key identity-constraint '{urn:t}K'.
== keyref ok
bool(true)
== keyref missing
bool(false)
  [2 1877] line 3: Element '{urn:t}ref': No match found for key-sequence ['c'] of keyref '{urn:t}R'.
== keyref dup key
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['a'] in key identity-constraint '{urn:t}K'.
== decimal dup
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['1.0'] in unique identity-constraint '{urn:t}N'.
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['1.5'] in unique identity-constraint '{urn:t}N'.
== element field dup
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['7'] in unique identity-constraint '{urn:t}E'.
== element field invalid
bool(false)
  [2 1824] line 1: Element '{urn:t}code': 'x' is not a valid value of the atomic type 'xs:integer'.
  [2 1877] line 1: Element '{urn:t}code': Warning: No precomputed value available, the value was either invalid or something strange happened.
== multi
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['a', '1'] in unique identity-constraint '{urn:t}M'.
== field more than one
bool(false)
  [2 1877] line 1: Element '{urn:t}sub', attribute 'v': The XPath 't:sub/@v' of a field of unique identity-constraint '{urn:t}S' evaluates to a node-set with more than one member.
== field non simple
bool(false)
  [2 1877] line 1: Element '{urn:t}sub': The XPath 't:sub' of a field of unique identity-constraint '{urn:t}NS' does evaluate to a node of non-simple type.
== default attr dup
bool(false)
  [2 1877] line 1: Element '{urn:t}item': Duplicate key-sequence ['dflt'] in unique identity-constraint '{urn:t}D'.
== union
bool(false)
  [2 1877] line 1: Element '{urn:t}ref': Duplicate key-sequence ['a'] in unique identity-constraint '{urn:t}UN'.
== descendant
bool(false)
  [2 1877] line 1: Element '{urn:t}sub': Duplicate key-sequence ['1'] in unique identity-constraint '{urn:t}DS'.
== bad selector
bool(false)
  [2 3037] line 23: Element '{http://www.w3.org/2001/XMLSchema}selector', attribute 'xpath': The XPath expression '@id' could not be compiled.
== bad prefix
bool(false)
  [2 3037] line 23: Element '{http://www.w3.org/2001/XMLSchema}selector', attribute 'xpath': The XPath expression 'q:item' could not be compiled.
== bad root
bool(false)
  [2 3037] line 23: Element '{http://www.w3.org/2001/XMLSchema}selector', attribute 'xpath': The XPath expression '/t:item' could not be compiled.
== keyref numeric
bool(false)
  [2 1877] line 1: Element '{urn:t}ref': No match found for key-sequence ['6'] of keyref '{urn:t}R2'.
== nested ok
bool(true)
== nested dup in group
bool(false)
  [2 1877] line 1: Element 'e': Duplicate key-sequence ['a'] in key identity-constraint 'GK'.
== nested across groups
bool(false)
  [2 1877] line 1: Element 'r': More than one match found for key-sequence ['a'] of keyref 'GR'.
  [2 1877] line 1: Element 'r': No match found for key-sequence ['z'] of keyref 'GR'.
