--TEST--
html_errors substitutes invalid UTF-8 in docref messages
--INI--
html_errors=1
--FILE--
<?php
fopen("\xfc\x63", "r");
--EXPECTF--
<br />
<b>Warning</b>:  fopen(�c): Failed to open stream: No such file or directory in <b>%s</b> on line <b>2</b><br />
