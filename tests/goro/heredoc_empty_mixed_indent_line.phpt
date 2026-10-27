--TEST--
Mixed indentation of an empty heredoc's closing label: on its line
--FILE--
<?php

echo <<<END
 	END;
--EXPECTF--

Parse error: Invalid indentation - tabs and spaces cannot be mixed in %s on line 4
