--TEST--
SoapClient::__soapCall(): $outputHeaders becomes an empty array right after parameter parsing
--FILE--
<?php
class C extends SoapClient {
  function __doRequest($req, $loc, $act, $ver, $one = false, ?string $uriParserClass = null): ?string { throw new Exception('stop'); }
}
$c = new C(null, ['location' => 'http://l.invalid/', 'uri' => 'urn:x']);
$out = 'old';
try { $c->__soapCall('f', [1], null, null, $out); } catch (Exception $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
var_dump($out);
$out2 = 5;
try { $c->__soapCall(5, 6, null, null, $out2); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($out2);
?>
--EXPECTF--
stop
#0 [internal function]: C->__doRequest('<?xml version="...', 'http://l.invali...', 'urn:x#f', 1, false)
#1 %s(7): SoapClient->__soapCall('f', Array, NULL, NULL, Array)
#2 {main}
array(0) {
}
SoapClient::__soapCall(): Argument #2 ($args) must be of type array, int given
int(5)
