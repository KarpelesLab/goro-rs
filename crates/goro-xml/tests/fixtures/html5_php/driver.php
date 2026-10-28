<?php
// Fixture generator for tests/html5_php.rs (run on the reference PHP 8.5):
//   php -d error_reporting=-1 -d display_errors=1 driver.php inputs.jsonl > php85.jsonl
// Each input (a JSON string per line; most come from html5lib-tests, MIT)
// is parsed with Dom\HTMLDocument::createFromString; the output line holds
// the libxml error messages and saveHtml().
libxml_use_internal_errors(true);
foreach (file($argv[1]) as $line) {
    $html = json_decode($line);
    libxml_clear_errors();
    $dom = Dom\HTMLDocument::createFromString($html);
    $errs = [];
    foreach (libxml_get_errors() as $e) {
        $errs[] = $e->message;
    }
    echo json_encode(['e' => $errs, 'h' => $dom->saveHtml()], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE), "\n";
}
