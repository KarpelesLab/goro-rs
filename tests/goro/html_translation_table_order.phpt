--TEST--
get_html_translation_table(): multi-code-point entities after their leading code point; charset traversal order
--FILE--
<?php
// get_html_translation_table(): multi-code-point HTML5 entities follow
// their leading code point's entity
foreach ([[HTML_ENTITIES, ENT_QUOTES | ENT_HTML5], [HTML_ENTITIES, ENT_QUOTES | ENT_HTML5, 'ISO-8859-1'], [HTML_ENTITIES],
          [HTML_SPECIALCHARS, ENT_HTML5], [HTML_ENTITIES, ENT_NOQUOTES | ENT_XHTML], [HTML_ENTITIES, ENT_QUOTES | ENT_HTML5, 'cp1252']] as $args) {
    $t = get_html_translation_table(...$args);
    echo count($t), ' ', md5(serialize($t)), "\n";
}
$t = get_html_translation_table(HTML_ENTITIES, ENT_QUOTES | ENT_HTML5);
$i = 0;
foreach ($t as $k => $v) {
    if (in_array($v, ['&lt;', '&nvlt;', '&fjlig;', '&MediumSpace;', '&ThickSpace;', '&sub;', '&vnsub;', '&nsubE;', '&subE;'])) echo $i, ' ', bin2hex($k), ' ', $v, "\n";
    $i++;
}
--EXPECT--
1511 622c2e4e6b3102b086773ddd3735fb54
132 7e689003b16bd16e1c9aa05eaf93de60
253 1ca0faafc69509297d049672567fd3c8
3 ba66a30692ca09aec3e447db0f509fcd
251 251920be301c6ce47b26c655beb9b8db
156 3875ffaa0a82042d34bcc6da54e696b3
18 3c &lt;
19 3ce28392 &nvlt;
32 666a &fjlig;
459 e2819f &MediumSpace;
460 e2819fe2808a &ThickSpace;
741 e28a82 &sub;
742 e28a82e28392 &vnsub;
1334 e2ab85 &subE;
1335 e2ab85ccb8 &nsubE;
