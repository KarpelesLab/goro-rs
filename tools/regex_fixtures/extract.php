<?php
// Extracts (pattern, subject) pairs from literal preg_* calls in .phpt files.
// Usage: php extract.php <tests-root> <out-file>
$root = $argv[1];
$out = fopen($argv[2], 'w');
$it = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($root, FilesystemIterator::SKIP_DOTS));
$seen = [];
foreach ($it as $f) {
    if (substr($f, -5) !== '.phpt') continue;
    $src = file_get_contents($f);
    if (strpos($src, 'preg_') === false) continue;
    if (!preg_match('/^--FILE--\R(.*?)^--[A-Z_]+--/sm', $src, $m)) continue;
    $code = $m[1];
    $toks = @token_get_all($code);
    $n = count($toks);
    for ($i = 0; $i < $n; $i++) {
        $t = $toks[$i];
        if (!is_array($t) || $t[0] !== T_STRING) continue;
        $fn = strtolower($t[1]);
        if (!in_array($fn, ['preg_match', 'preg_match_all', 'preg_replace', 'preg_split', 'preg_replace_callback', 'preg_grep'])) continue;
        // collect top-level args
        $j = $i + 1;
        while ($j < $n && is_array($toks[$j]) && $toks[$j][0] === T_WHITESPACE) $j++;
        if ($j >= $n || $toks[$j] !== '(') continue;
        $depth = 0; $args = []; $cur = [];
        for ($k = $j + 1; $k < $n; $k++) {
            $x = $toks[$k];
            if ($x === '(' || $x === '[' || $x === '{') $depth++;
            if ($x === ')' || $x === ']' || $x === '}') {
                if ($depth === 0) { $args[] = $cur; break; }
                $depth--;
            }
            if ($x === ',' && $depth === 0) { $args[] = $cur; $cur = []; continue; }
            if (is_array($x) && in_array($x[0], [T_WHITESPACE, T_COMMENT])) continue;
            $cur[] = $x;
        }
        $lit = function ($a) {
            if ($a === null || count($a) !== 1 || !is_array($a[0]) || $a[0][0] !== T_CONSTANT_ENCAPSED_STRING) return null;
            try { return eval('return ' . $a[0][1] . ';'); } catch (Throwable $e) { return null; }
        };
        $pat = $lit($args[0] ?? null);
        if ($pat === null) continue;
        $subjIdx = in_array($fn, ['preg_replace', 'preg_replace_callback']) ? 2 : 1;
        $subj = $lit($args[$subjIdx] ?? null);
        if ($subj === null) $subj = '';
        $key = $pat . "\0" . $subj;
        if (isset($seen[$key])) continue;
        $seen[$key] = 1;
        fwrite($out, bin2hex($pat) . "\t" . bin2hex($subj) . "\n");
    }
}
fclose($out);
echo count($seen), " cases\n";
