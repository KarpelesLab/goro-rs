--TEST--
Internal classes only their functions create cannot be constructed directly
--FILE--
<?php
foreach (["Socket","AddressInfo","XMLParser","DeflateContext","InflateContext","OpenSSLCertificate","OpenSSLCertificateSigningRequest","OpenSSLAsymmetricKey","Soap\\Url","Soap\\Sdl"] as $c) { try { new $c(1, 2); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$r = new ReflectionClass("Socket"); try { $r->newInstance(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($r->isInstantiable());
--EXPECT--
Error: Cannot directly construct Socket, use socket_create() instead
Error: Cannot directly construct AddressInfo, use socket_addrinfo_lookup() instead
Error: Cannot directly construct XMLParser, use xml_parser_create() or xml_parser_create_ns() instead
Error: Cannot directly construct DeflateContext, use deflate_init() instead
Error: Cannot directly construct InflateContext, use inflate_init() instead
Error: Cannot directly construct OpenSSLCertificate, use openssl_x509_read() instead
Error: Cannot directly construct OpenSSLCertificateSigningRequest, use openssl_csr_new() instead
Error: Cannot directly construct OpenSSLAsymmetricKey, use openssl_pkey_new() instead
Error: Cannot directly construct Soap\Url
Error: Cannot directly construct Soap\Sdl
Cannot directly construct Socket, use socket_create() instead
bool(true)
