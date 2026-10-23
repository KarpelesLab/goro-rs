--TEST--
serialize()/var_export() of exceptions: declared visibility and slot order of the base properties
--FILE--
<?php
class E1 extends Exception {
    protected $message = 'dm';
    public function __construct() { $this->file = 'f'; $this->line = 1; }
}
class E2 extends Exception {
    private $x = 1;
    public $code = 5;
    public function __construct() { $this->file = 'f'; $this->line = 1; }
}
foreach ([new ErrorException('e', 1, 2, 'f', 3), new E1, new E2] as $o) {
    echo str_replace("\0", '~', serialize($o)), "\n";
    echo str_replace("\0", '~', var_export($o, true)), "\n";
    $u = unserialize(serialize($o));
    echo get_class($u), ' ', $u->getMessage(), ' ', $u->getCode(), "\n";
}
var_dump(unserialize(serialize(new ErrorException('e', 1, 2, 'f', 3)))->getSeverity());
--EXPECT--
O:14:"ErrorException":8:{s:10:"~*~message";s:1:"e";s:17:"~Exception~string";s:0:"";s:7:"~*~code";i:1;s:7:"~*~file";s:1:"f";s:7:"~*~line";i:3;s:16:"~Exception~trace";a:0:{}s:19:"~Exception~previous";N;s:11:"~*~severity";i:2;}
\ErrorException::__set_state(array(
   'message' => 'e',
   'string' => '',
   'code' => 1,
   'file' => 'f',
   'line' => 3,
   'trace' => 
  array (
  ),
   'previous' => NULL,
   'severity' => 2,
))
ErrorException e 1
O:2:"E1":7:{s:10:"~*~message";s:2:"dm";s:17:"~Exception~string";s:0:"";s:7:"~*~code";i:0;s:7:"~*~file";s:1:"f";s:7:"~*~line";i:1;s:16:"~Exception~trace";a:0:{}s:19:"~Exception~previous";N;}
\E1::__set_state(array(
   'message' => 'dm',
   'string' => '',
   'code' => 0,
   'file' => 'f',
   'line' => 1,
   'trace' => 
  array (
  ),
   'previous' => NULL,
))
E1 dm 0
O:2:"E2":8:{s:10:"~*~message";s:0:"";s:17:"~Exception~string";s:0:"";s:4:"code";i:5;s:7:"~*~file";s:1:"f";s:7:"~*~line";i:1;s:16:"~Exception~trace";a:0:{}s:19:"~Exception~previous";N;s:5:"~E2~x";i:1;}
\E2::__set_state(array(
   'message' => '',
   'string' => '',
   'code' => 5,
   'file' => 'f',
   'line' => 1,
   'trace' => 
  array (
  ),
   'previous' => NULL,
   'x' => 1,
))
E2  5
int(2)
