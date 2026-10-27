--TEST--
XSLTProcessor: each failing php:function() call of a transformation chains its Error; a failed registerPHPFunctions() keeps the namespace and earlier entries
--FILE--
<?php
function show($f) { try { var_dump($f()); } catch (Throwable $e) { do { echo get_class($e), ": ", $e->getMessage(), "\n"; } while ($e = $e->getPrevious()); } }
function up($s) { return strtoupper($s); }
$xml = new DOMDocument; $xml->loadXML('<r/>');
$xsl = new DOMDocument; $xsl->loadXML('<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:php="http://php.net/xsl"><xsl:output method="text"/><xsl:template match="/">[<xsl:value-of select="php:function(\'up\', \'x\')"/>|<xsl:value-of select="php:function(\'lo\', \'Y\')"/>|<xsl:value-of select="php:function(\'up\', \'z\')"/>]</xsl:template></xsl:stylesheet>');
$p = new XSLTProcessor; $p->importStylesheet($xsl);
show(fn() => $p->transformToXml($xml));
show(fn() => $p->registerPHPFunctions('nope'));
show(fn() => $p->transformToXml($xml));
$p = new XSLTProcessor; $p->importStylesheet($xsl);
show(fn() => $p->registerPHPFunctions(['up', 'nope']));
show(fn() => $p->transformToXml($xml));
$p = new XSLTProcessor; $p->importStylesheet($xsl);
show(fn() => $p->registerPHPFunctions(['up']));
show(fn() => $p->transformToXml($xml));
?>
--EXPECT--
Error: No callbacks were registered
Error: No callbacks were registered
Error: No callbacks were registered
TypeError: XSLTProcessor::registerPHPFunctions(): Argument #1 ($functions) must be a callable, function "nope" not found or invalid function name
Error: No callback handler "up" registered
Error: No callback handler "lo" registered
Error: No callback handler "up" registered
TypeError: XSLTProcessor::registerPHPFunctions(): Argument #1 ($functions) must be an array with valid callbacks as values, function "nope" not found or invalid function name
Error: No callback handler "lo" registered
NULL
Error: No callback handler "lo" registered
