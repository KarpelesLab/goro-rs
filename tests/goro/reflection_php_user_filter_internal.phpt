--TEST--
php_user_filter methods are internal; ReflectionFunction::IS_DEPRECATED
--FILE--
<?php
foreach (['filter', 'onCreate', 'onClose'] as $m) {
  $r = new ReflectionMethod('php_user_filter', $m);
  var_dump($r->isInternal(), $r->isUserDefined(), $r->getFileName());
  echo $r;
}
var_dump(ReflectionFunction::IS_DEPRECATED);
--EXPECT--
bool(true)
bool(false)
bool(false)
Method [ <internal:standard> public method filter ] {

  - Parameters [4] {
    Parameter #0 [ <required> $in ]
    Parameter #1 [ <required> $out ]
    Parameter #2 [ <required> &$consumed ]
    Parameter #3 [ <required> bool $closing ]
  }
  - Tentative return [ int ]
}
bool(true)
bool(false)
bool(false)
Method [ <internal:standard> public method onCreate ] {

  - Parameters [0] {
  }
  - Tentative return [ bool ]
}
bool(true)
bool(false)
bool(false)
Method [ <internal:standard> public method onClose ] {

  - Parameters [0] {
  }
  - Tentative return [ void ]
}
int(2048)
