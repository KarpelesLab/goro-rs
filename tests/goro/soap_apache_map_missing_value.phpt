--TEST--
SOAP apache Map decoding: an item without a value element is a missing value error
--FILE--
<?php
class C extends SoapClient { public $resp; function __doRequest($req, $loc, $act, $ver, $one = false, ?string $uriParserClass = null): ?string { return $this->resp; } }
$c = new C(null, ['location' => 'http://l.invalid/', 'uri' => 'urn:x']);
foreach (['<item><key xsi:type="xsd:string">b</key><other>1</other></item>', '<item><key xsi:type="xsd:string">b</key></item>', '<item><value>1</value></item>', '<item><key xsi:type="xsd:string">b</key><value>1</value></item>'] as $items) {
$c->resp = '<?xml version="1.0"?><SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/" xmlns:ns1="urn:x" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:ns2="http://xml.apache.org/xml-soap"><SOAP-ENV:Body><ns1:xResponse><p xsi:type="ns2:Map">' . $items . '</p></ns1:xResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>';
try { var_dump($c->x()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
?>
--EXPECT--
SoapFault: SOAP-ERROR: Encoding: Can't decode apache map, missing value
SoapFault: SOAP-ERROR: Encoding: Can't decode apache map, missing value
SoapFault: SOAP-ERROR: Encoding: Can't decode apache map, missing key
array(1) {
  ["b"]=>
  string(1) "1"
}
