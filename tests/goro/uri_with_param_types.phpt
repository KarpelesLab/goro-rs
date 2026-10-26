--TEST--
Uri\Rfc3986\Uri / Uri\WhatWg\Url withers and resolve() check their parameter types
--FILE--
<?php
function t($f) { try { $r = $f(); echo is_object($r) ? $r->toString() ?? '' : var_export($r, true), "\n"; } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
function u($f) { try { $r = $f(); echo is_object($r) ? $r->toAsciiString() : var_export($r, true), "\n"; } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$r = new Uri\Rfc3986\Uri('http://h/p');
$w = new Uri\WhatWg\Url('http://h/p');
foreach (['withScheme', 'withUserInfo', 'withHost', 'withPort', 'withPath', 'withQuery', 'withFragment', 'resolve'] as $m) {
    foreach ([[], null, new stdClass, 'x y', '81', 1.5, true] as $v) {
        echo "R $m ", gettype($v), ": "; t(fn() => $r->$m($v));
    }
}
foreach (['withScheme', 'withUsername', 'withPassword', 'withHost', 'withPort', 'withPath', 'withQuery', 'withFragment', 'resolve'] as $m) {
    foreach ([[], null, new stdClass, 'x y', '81', 1.5, true] as $v) {
        echo "W $m ", gettype($v), ": "; u(fn() => $w->$m($v));
    }
}
t(fn() => $r->withHost());
t(fn() => $r->withHost('a', 'b'));
u(fn() => $w->resolve('/x', []));
--EXPECTF--
R withScheme array: TypeError: Uri\Rfc3986\Uri::withScheme(): Argument #1 ($scheme) must be of type ?string, array given
R withScheme NULL: //h/p
R withScheme object: TypeError: Uri\Rfc3986\Uri::withScheme(): Argument #1 ($scheme) must be of type ?string, stdClass given
R withScheme string: Uri\InvalidUriException: The specified scheme is malformed
R withScheme string: Uri\InvalidUriException: The specified scheme is malformed
R withScheme double: Uri\InvalidUriException: The specified scheme is malformed
R withScheme boolean: Uri\InvalidUriException: The specified scheme is malformed
R withUserInfo array: TypeError: Uri\Rfc3986\Uri::withUserInfo(): Argument #1 ($userinfo) must be of type ?string, array given
R withUserInfo NULL: http://h/p
R withUserInfo object: TypeError: Uri\Rfc3986\Uri::withUserInfo(): Argument #1 ($userinfo) must be of type ?string, stdClass given
R withUserInfo string: Uri\InvalidUriException: The specified userinfo is malformed
R withUserInfo string: http://81@h/p
R withUserInfo double: http://1.5@h/p
R withUserInfo boolean: http://1@h/p
R withHost array: TypeError: Uri\Rfc3986\Uri::withHost(): Argument #1 ($host) must be of type ?string, array given
R withHost NULL: http:/p
R withHost object: TypeError: Uri\Rfc3986\Uri::withHost(): Argument #1 ($host) must be of type ?string, stdClass given
R withHost string: Uri\InvalidUriException: The specified host is malformed
R withHost string: http://81/p
R withHost double: http://1.5/p
R withHost boolean: http://1/p
R withPort array: TypeError: Uri\Rfc3986\Uri::withPort(): Argument #1 ($port) must be of type ?int, array given
R withPort NULL: http://h/p
R withPort object: TypeError: Uri\Rfc3986\Uri::withPort(): Argument #1 ($port) must be of type ?int, stdClass given
R withPort string: TypeError: Uri\Rfc3986\Uri::withPort(): Argument #1 ($port) must be of type ?int, string given
R withPort string: http://h:81/p
R withPort double: 
Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 8
http://h:1/p
R withPort boolean: http://h:1/p
R withPath array: TypeError: Uri\Rfc3986\Uri::withPath(): Argument #1 ($path) must be of type string, array given
R withPath NULL: 
Deprecated: Uri\Rfc3986\Uri::withPath(): Passing null to parameter #1 ($path) of type string is deprecated in %s on line 8
http://h
R withPath object: TypeError: Uri\Rfc3986\Uri::withPath(): Argument #1 ($path) must be of type string, stdClass given
R withPath string: Uri\InvalidUriException: The specified path is malformed
R withPath string: Uri\InvalidUriException: The specified path is malformed
R withPath double: Uri\InvalidUriException: The specified path is malformed
R withPath boolean: Uri\InvalidUriException: The specified path is malformed
R withQuery array: TypeError: Uri\Rfc3986\Uri::withQuery(): Argument #1 ($query) must be of type ?string, array given
R withQuery NULL: http://h/p
R withQuery object: TypeError: Uri\Rfc3986\Uri::withQuery(): Argument #1 ($query) must be of type ?string, stdClass given
R withQuery string: Uri\InvalidUriException: The specified query is malformed
R withQuery string: http://h/p?81
R withQuery double: http://h/p?1.5
R withQuery boolean: http://h/p?1
R withFragment array: TypeError: Uri\Rfc3986\Uri::withFragment(): Argument #1 ($fragment) must be of type ?string, array given
R withFragment NULL: http://h/p
R withFragment object: TypeError: Uri\Rfc3986\Uri::withFragment(): Argument #1 ($fragment) must be of type ?string, stdClass given
R withFragment string: Uri\InvalidUriException: The specified fragment is malformed
R withFragment string: http://h/p#81
R withFragment double: http://h/p#1.5
R withFragment boolean: http://h/p#1
R resolve array: TypeError: Uri\Rfc3986\Uri::resolve(): Argument #1 ($uri) must be of type string, array given
R resolve NULL: 
Deprecated: Uri\Rfc3986\Uri::resolve(): Passing null to parameter #1 ($uri) of type string is deprecated in %s on line 8
http://h/p
R resolve object: TypeError: Uri\Rfc3986\Uri::resolve(): Argument #1 ($uri) must be of type string, stdClass given
R resolve string: Uri\InvalidUriException: The specified URI is malformed
R resolve string: http://h/81
R resolve double: http://h/1.5
R resolve boolean: http://h/1
W withScheme array: TypeError: Uri\WhatWg\Url::withScheme(): Argument #1 ($scheme) must be of type string, array given
W withScheme NULL: 
Deprecated: Uri\WhatWg\Url::withScheme(): Passing null to parameter #1 ($scheme) of type string is deprecated in %s on line 13
Uri\WhatWg\InvalidUrlException: The specified scheme is malformed
W withScheme object: TypeError: Uri\WhatWg\Url::withScheme(): Argument #1 ($scheme) must be of type string, stdClass given
W withScheme string: Uri\WhatWg\InvalidUrlException: The specified scheme is malformed
W withScheme string: Uri\WhatWg\InvalidUrlException: The specified scheme is malformed
W withScheme double: Uri\WhatWg\InvalidUrlException: The specified scheme is malformed
W withScheme boolean: Uri\WhatWg\InvalidUrlException: The specified scheme is malformed
W withUsername array: TypeError: Uri\WhatWg\Url::withUsername(): Argument #1 ($username) must be of type ?string, array given
W withUsername NULL: http://h/p
W withUsername object: TypeError: Uri\WhatWg\Url::withUsername(): Argument #1 ($username) must be of type ?string, stdClass given
W withUsername string: http://x%20y@h/p
W withUsername string: http://81@h/p
W withUsername double: http://1.5@h/p
W withUsername boolean: http://1@h/p
W withPassword array: TypeError: Uri\WhatWg\Url::withPassword(): Argument #1 ($password) must be of type ?string, array given
W withPassword NULL: http://h/p
W withPassword object: TypeError: Uri\WhatWg\Url::withPassword(): Argument #1 ($password) must be of type ?string, stdClass given
W withPassword string: http://:x%20y@h/p
W withPassword string: http://:81@h/p
W withPassword double: http://:1.5@h/p
W withPassword boolean: http://:1@h/p
W withHost array: TypeError: Uri\WhatWg\Url::withHost(): Argument #1 ($host) must be of type ?string, array given
W withHost NULL: Uri\WhatWg\InvalidUrlException: The specified host is malformed (HostMissing)
W withHost object: TypeError: Uri\WhatWg\Url::withHost(): Argument #1 ($host) must be of type ?string, stdClass given
W withHost string: Uri\WhatWg\InvalidUrlException: The specified host is malformed (DomainInvalidCodePoint)
W withHost string: http://0.0.0.81/p
W withHost double: http://1.0.0.5/p
W withHost boolean: http://0.0.0.1/p
W withPort array: TypeError: Uri\WhatWg\Url::withPort(): Argument #1 ($port) must be of type ?int, array given
W withPort NULL: http://h/p
W withPort object: TypeError: Uri\WhatWg\Url::withPort(): Argument #1 ($port) must be of type ?int, stdClass given
W withPort string: TypeError: Uri\WhatWg\Url::withPort(): Argument #1 ($port) must be of type ?int, string given
W withPort string: http://h:81/p
W withPort double: 
Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 13
http://h:1/p
W withPort boolean: http://h:1/p
W withPath array: TypeError: Uri\WhatWg\Url::withPath(): Argument #1 ($path) must be of type string, array given
W withPath NULL: 
Deprecated: Uri\WhatWg\Url::withPath(): Passing null to parameter #1 ($path) of type string is deprecated in %s on line 13
http://h/
W withPath object: TypeError: Uri\WhatWg\Url::withPath(): Argument #1 ($path) must be of type string, stdClass given
W withPath string: http://h/x%20y
W withPath string: http://h/81
W withPath double: http://h/1.5
W withPath boolean: http://h/1
W withQuery array: TypeError: Uri\WhatWg\Url::withQuery(): Argument #1 ($query) must be of type ?string, array given
W withQuery NULL: http://h/p
W withQuery object: TypeError: Uri\WhatWg\Url::withQuery(): Argument #1 ($query) must be of type ?string, stdClass given
W withQuery string: http://h/p?x%20y
W withQuery string: http://h/p?81
W withQuery double: http://h/p?1.5
W withQuery boolean: http://h/p?1
W withFragment array: TypeError: Uri\WhatWg\Url::withFragment(): Argument #1 ($fragment) must be of type ?string, array given
W withFragment NULL: http://h/p
W withFragment object: TypeError: Uri\WhatWg\Url::withFragment(): Argument #1 ($fragment) must be of type ?string, stdClass given
W withFragment string: http://h/p#x%20y
W withFragment string: http://h/p#81
W withFragment double: http://h/p#1.5
W withFragment boolean: http://h/p#1
W resolve array: TypeError: Uri\WhatWg\Url::resolve(): Argument #1 ($uri) must be of type string, array given
W resolve NULL: 
Deprecated: Uri\WhatWg\Url::resolve(): Passing null to parameter #1 ($uri) of type string is deprecated in %s on line 13
http://h/p
W resolve object: TypeError: Uri\WhatWg\Url::resolve(): Argument #1 ($uri) must be of type string, stdClass given
W resolve string: http://h/x%20y
W resolve string: http://h/81
W resolve double: http://h/1.5
W resolve boolean: http://h/1
ArgumentCountError: Uri\Rfc3986\Uri::withHost() expects exactly 1 argument, 0 given
ArgumentCountError: Uri\Rfc3986\Uri::withHost() expects exactly 1 argument, 2 given
Error: Uri\WhatWg\Url::resolve(): Argument #2 ($softErrors) could not be passed by reference
