<?php
// Shared helpers for the fixture drivers: a lossless, line-safe quoting of
// byte strings, and renderings of Url/Uri results.

function q($s) {
    if ($s === null) return 'N';
    if (is_int($s)) return (string)$s;
    if (is_bool($s)) return $s ? 'T' : 'F';
    $o = '"';
    $n = strlen($s);
    for ($i = 0; $i < $n; $i++) {
        $c = $s[$i];
        $b = ord($c);
        if ($b < 0x20 || $b >= 0x7f || $c === '"' || $c === '\\' || $c === '|') {
            $o .= sprintf('\\x%02X', $b);
        } else {
            $o .= $c;
        }
    }
    return $o . '"';
}

function errs(array $errors) {
    $p = [];
    foreach ($errors as $e) {
        $p[] = $e->type->name . ',' . ($e->failure ? '1' : '0') . ',' . q($e->context);
    }
    return '[' . implode(';', $p) . ']';
}

function wurl($u) {
    if ($u === null) return 'NULL';
    return implode('|', [
        q($u->getScheme()), q($u->getUsername()), q($u->getPassword()),
        q($u->getAsciiHost()), q($u->getUnicodeHost()), q($u->getPort()),
        q($u->getPath()), q($u->getQuery()), q($u->getFragment()),
        q($u->toAsciiString()), q($u->toUnicodeString()),
    ]);
}

function ruri($u) {
    if ($u === null) return 'NULL';
    return implode('|', [
        q($u->getScheme()), q($u->getRawScheme()),
        q($u->getUsername()), q($u->getRawUsername()),
        q($u->getPassword()), q($u->getRawPassword()),
        q($u->getUserInfo()), q($u->getRawUserInfo()),
        q($u->getHost()), q($u->getRawHost()), q($u->getPort()),
        q($u->getPath()), q($u->getRawPath()),
        q($u->getQuery()), q($u->getRawQuery()),
        q($u->getFragment()), q($u->getRawFragment()),
        q($u->toString()), q($u->toRawString()),
    ]);
}

function exc(Throwable $e) {
    $s = 'EXC ' . get_class($e) . ' ' . q($e->getMessage());
    if ($e instanceof Uri\WhatWg\InvalidUrlException) {
        $s .= ' ' . errs($e->errors);
    }
    return $s;
}
