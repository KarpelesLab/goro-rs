--TEST--
SOAP double encoding is zend_gcvt() at precision: -0, integer ties, INF cut to precision
--FILE--
<?php
class C extends SoapClient { function __doRequest($req, $loc, $act, $ver, $one = false, ?string $uriParserClass = null): ?string { echo $req, "\n"; return ''; } }
$c = new C(null, ['location' => 'http://l.invalid/', 'uri' => 'urn:x', 'style' => SOAP_DOCUMENT, 'use' => SOAP_LITERAL]);
$c->__soapCall('op', [-0.0, 1.5, 100000000000005.0, -1e-7, NAN, -INF]);
ini_set('precision', 2);
$c->__soapCall('op', [1005.0, 0.125, INF]);
?>
--EXPECT--
<?xml version="1.0" encoding="UTF-8"?>
<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/"><SOAP-ENV:Body><param0>-0</param0><param1>1.5</param1><param2>1.0000000000000E+14</param2><param3>-1.0E-7</param3><param4>NAN</param4><param5>-INF</param5></SOAP-ENV:Body></SOAP-ENV:Envelope>

<?xml version="1.0" encoding="UTF-8"?>
<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/"><SOAP-ENV:Body><param0>1.0E+3</param0><param1>0.12</param1><param2>IN</param2></SOAP-ENV:Body></SOAP-ENV:Envelope>
