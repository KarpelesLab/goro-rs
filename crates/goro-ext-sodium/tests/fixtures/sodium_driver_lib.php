<?php
// Helpers shared by the fixture drivers (see sodium_driver.php for the
// line format).
$W = [];
set_error_handler(function ($no, $msg) { $GLOBALS['W'][] = $msg; return true; });
function warns() { $w = ''; foreach ($GLOBALS['W'] as $m) $w .= 'W' . $m . '|'; $GLOBALS['W'] = []; return $w; }
function rb($n) { $s = ''; for ($i = 0; $i < $n; $i++) $s .= chr(mt_rand(0, 255)); return $s; }
function tok($v) {
    if (is_string($v)) return 's' . bin2hex($v);
    if (is_int($v)) return 'i' . $v;
    if (is_null($v)) return 'n';
    if (is_array($v)) return 'a';
    if (is_bool($v)) return $v ? 'B1' : 'B0';
    return '?';
}
function res($r) {
    if (is_string($r)) return 'S' . bin2hex($r);
    if (is_bool($r)) return $r ? 'B1' : 'B0';
    if (is_int($r)) return 'I' . $r;
    if (is_null($r)) return 'N';
    if (is_array($r)) return 'A' . implode(',', array_map('res', $r));
    return '?';
}
function emit($f, $args, $out) { echo $f, "\t", implode(' ', array_map('tok', $args)), "\t=>\t", warns(), $out, "\n"; }
// Calls $f with $args; returns the result (or null on exception) and prints the line.
function c($f, ...$args) {
    try {
        $r = $f(...$args);
        emit($f, $args, res($r));
        return $r;
    } catch (Throwable $e) {
        emit($f, $args, 'E' . get_class($e) . ':' . $e->getMessage());
        return null;
    }
}
// By-reference first argument.
function cr($f, $ref, ...$rest) {
    $orig = $ref;
    try {
        $r = $f($ref, ...$rest);
        $out = $r === null ? 'V' : res($r);
    } catch (Throwable $e) {
        $out = 'E' . get_class($e) . ':' . $e->getMessage();
    }
    emit($f, array_merge([$orig], $rest), $out . ' R' . tok($ref));
    return $ref;
}
function len($f, ...$args) {
    try { $r = $f(...$args); $o = is_array($r) ? implode(',', array_map('strlen', $r)) : strlen($r); }
    catch (Throwable $e) { $o = 'E' . get_class($e) . ':' . $e->getMessage(); }
    echo "#len\t", $f, "\t", implode(' ', array_map('tok', $args)), "\t=>\t", warns(), $o, "\n";
}
// Valid lengths L: try 0, L-1, L+1.
function bad($l) { return array_unique([0, max(0, $l - 1), $l + 1]); }
