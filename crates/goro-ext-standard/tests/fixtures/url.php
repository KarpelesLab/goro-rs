<?php
// Fixture driver for url.rs: run once on PHP 8.5 (observed behaviour).
//   php85 -d error_reporting=-1 -d display_errors=1 -d log_errors=0 -d html_errors=0 url.php URLS_INC > url_full.txt
// argv[1]: ext/standard/tests/url/urls.inc of the php-src test suite (its
// $urls list is used as input).
require __DIR__ . '/common.inc';

$all = '';
for ($i = 0; $i < 256; $i++) $all .= chr($i);
label($all, 'bytes:0-256');

// --- urlencode family
foreach (['urlencode', 'rawurlencode', 'urldecode', 'rawurldecode'] as $f) {
    run($f, $all);
    run($f, '');
    run($f, 'a b+c%20d%2x%%41%4%');
}
$ualpha = ['%', '%', '+', 'a', 'F', 'f', '0', '9', 'g', ' ', '%2', '%41', '%zz', '%%', '~', '.', "\xff", "\0"];
for ($i = 0; $i < 400; $i++) {
    $s = rnd_str($ualpha, 8);
    run('urldecode', $s);
    run('rawurldecode', $s);
}

// --- http_build_query
$arrays = [
    ["foo" => "bar", "baz" => 1, "test" => "a ' \" ", "abc", 'true' => true, 'false' => false],
    [20, 5 => 13, "9" => [1 => "val1", 3 => "val2", "string" => "string"], "name" => "homepage", "page" => 10,
        "sort" => ["desc", "admin" => ["admin1", "admin2" => ["who" => "admin2", 2 => "test"]]]],
    ['a' => null, 'b' => 'x', 'c' => [], 'd' => [null], 'e' => ['f' => null, 'g' => 'h']],
    ['k y' => 'v~a.l_u-e', 'é' => "\xff\x00", '[x]' => '[y]', -5 => -7, 'n' => ['a b' => ['c~d' => 1]]],
    [],
    [null],
    [0 => 'a', 1 => ['b', 'c'], 'x' => [[['deep']]]],
];
foreach ($arrays as $a) {
    run('http_build_query', $a);
    run('http_build_query', $a, 'pre_');
    run('http_build_query', $a, 'p r', ';');
    run('http_build_query', $a, '', '&amp;', PHP_QUERY_RFC3986);
    run('http_build_query', $a, 'n~', null, PHP_QUERY_RFC1738);
    run('http_build_query', $a, 'n~', '', PHP_QUERY_RFC3986);
    run('http_build_query', $a, '', '&', 0);
    run('http_build_query', $a, '', '&', 3);
    run('http_build_query', $a, '', '&', 99);
}

// --- parse_url
include $argv[1];
foreach ($urls as $u) {
    run('parse_url', $u);
    foreach ([-1, 0, 1, 2, 3, 4, 5, 6, 7] as $c) run('parse_url', $u, $c);
}
foreach (['//example.org', ':', '::', ':80', ':80/', '//example.org:81/hi?a=b#c=d', 'http://example.com:80#@google.com/',
    'http://example.com:80?@google.com/', '//php.net/path?query=1:2', '//username@php.net/path?query=1:2',
    'http://user:pass@host', '//user:pass@host', '//user@host', '/busca/?fq=B:20001', '/busca/?fq=B:200013',
    '/busca/?fq=home:01234', '/busca/?fq=home:012345', 'http://example.com/path/script.html?t=1#fragment?data',
    "http://a\x01b/c\x7fd?e\x1ff#g\x00h", "a\x01b:c", "http://u\x01:p\x02@h/", 'a:65535', 'a:65536', 'a:01/',
    'a:0', 'host:123456', 'http://h:65535', 'http://h:65536', 'http://h:080', 'http://h:8x', 'http://h:x8',
    'http://h: 8', 'http://h:-1', 'http://[::1]:80/', 'http://[::1/', 'http://]:80', '?a', '#a', '?', '#', '/?#',
    'FILE:///C:/x', 'file:///C:/x', 'FiLe:///c:', 'file:////x', 'file://', 'file:', 'http:', 'http:/', 'http://',
    'x:y:z', 'a+b.c-d:e', '1a:b', '.a:b', '-a:b', 'a_b:c', ' http://x', "http://x\n", 'http://x y/z w',
    'http://@x', 'http://a@b@c', 'http://a:b:c@d', 'http://:@x', 'http://x:', 'http://x:/', 'http://x:#',
    'mailto:a@b?subject=c', 'news:comp.lang', 'tel:+1-816-555-1212', 'urn:oasis:names', 'x://y', 'x:///y',
    'x:////y', '///x', '////x', '/\\x', 'http:\\\\x', 'http://x\\y'] as $u) {
    run('parse_url', $u);
}
foreach ([-2, 8, 99, PHP_INT_MAX, PHP_INT_MIN] as $c) run('parse_url', 'http://x', $c);
$palpha = ['http', 'file', 'x', 'FILE', ':', ':', '/', '/', '//', '?', '#', '@', '[', ']', '.', '1', '8', '0',
    '65535', '65536', 'a', 'b', '%', ' ', "\x01", "\x7f", ':80', 'u:p@', '~', '+', '-', '\\', '::1', '://'];
for ($i = 0; $i < 4000; $i++) {
    run('parse_url', rnd_str($palpha, 7));
}

// --- base64
for ($n = 0; $n <= 10; $n++) run('base64_encode', substr("\x00\xff\xfe\x01\x80\x7fabc\xc3\xa9xyz", 0, $n));
run('base64_encode', $all);
run('base64_decode', base64_encode($all));
run('base64_decode', base64_encode($all), true);
foreach (['=VGhl=ICc9=JyBz=eW1i=b2xz=IGFy=ZW4n=dCBh=bGxv=d2Vk=IHdo=ZXJl=IGkg=cHV0=IHRo=ZW0g=by5P', "UEhQ\n",
    "UEg=\n", "UA==\n", "UA==\r\n", "UA==\t", "UA== ", " UA==", "U A = =", "UA=", "UA", "U", "UA===", "UA==A",
    "UEhQ=", "UEhQ==", "U===", "====", "=", "", "UE\0hQ", "YQ", "YWI", "YWJj", "YW=Jj"] as $s) {
    run('base64_decode', $s);
    run('base64_decode', $s, true);
}
$balpha = ['A', 'Q', 'g', 'w', '=', '=', ' ', "\n", "\t", "\r", '*', '-', '_', '/', '+', "\0", 'Z', 'z', '9', "\xff", '.'];
for ($i = 0; $i < 1500; $i++) {
    $s = rnd_str($balpha, 10);
    run('base64_decode', $s);
    run('base64_decode', $s, true);
}
echo "# done\n";
