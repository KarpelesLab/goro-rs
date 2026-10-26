--TEST--
A parent named through a class_alias() is the aliased class; parameter types naming an alias accept subclasses (hoa/stream in phpstan.phar)
--FILE--
<?php
// hoa/stream (inside phpstan.phar): class Generic extends Stream, where
// `use Hoa\Stream;` names the class_alias Hoa\Stream of Hoa\Stream\Stream
namespace Hoa\Stream {
    interface IStream {}
    abstract class Stream implements IStream {
        public static function take(\Hoa\Stream\Stream $h) { return get_class($h); }
        public function self() { return self::take($this); }
    }
    class_alias('Hoa\Stream\Stream', 'Hoa\Stream');
}
namespace Hoa\File {
    use Hoa\Stream;
    abstract class Generic extends Stream {}
    abstract class File extends Generic {}
    class Read extends File {}
    $r = new Read;
    var_dump($r instanceof \Hoa\Stream\Stream, $r instanceof Stream, is_a($r, 'Hoa\Stream\Stream'), is_subclass_of($r, 'Hoa\Stream'));
    var_dump(get_parent_class(Generic::class), class_parents($r));
    var_dump($r->self());
    function f(Stream $s): Stream { return $s; }
    var_dump(get_class(f($r)));
    var_dump((new \ReflectionClass($r))->isSubclassOf('Hoa\Stream\Stream'));
}

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
string(17) "Hoa\Stream\Stream"
array(3) {
  ["Hoa\File\File"]=>
  string(13) "Hoa\File\File"
  ["Hoa\File\Generic"]=>
  string(16) "Hoa\File\Generic"
  ["Hoa\Stream\Stream"]=>
  string(17) "Hoa\Stream\Stream"
}
string(13) "Hoa\File\Read"
string(13) "Hoa\File\Read"
bool(true)
