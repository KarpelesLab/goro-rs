--TEST--
Internal classes list inherited constants first (constants_table order); no ZEND_DEBUG-only methods
--FILE--
<?php
foreach (['RecursiveArrayIterator', 'RecursiveTreeIterator', 'Phar', 'SplTempFileObject', 'DateTimeImmutable'] as $c) {
  echo $c, ": ", implode(',', array_keys((new ReflectionClass($c))->getConstants())), "\n";
}
echo new ReflectionClass('RecursiveArrayIterator');
var_dump(method_exists('Dom\HTMLDocument', 'debugGetTemplateCount'));
--EXPECT--
RecursiveArrayIterator: STD_PROP_LIST,ARRAY_AS_PROPS,CHILD_ARRAYS_ONLY
RecursiveTreeIterator: LEAVES_ONLY,SELF_FIRST,CHILD_FIRST,CATCH_GET_CHILD,BYPASS_CURRENT,BYPASS_KEY,PREFIX_LEFT,PREFIX_MID_HAS_NEXT,PREFIX_MID_LAST,PREFIX_END_HAS_NEXT,PREFIX_END_LAST,PREFIX_RIGHT
Phar: CURRENT_MODE_MASK,CURRENT_AS_PATHNAME,CURRENT_AS_FILEINFO,CURRENT_AS_SELF,KEY_MODE_MASK,KEY_AS_PATHNAME,FOLLOW_SYMLINKS,KEY_AS_FILENAME,NEW_CURRENT_AND_KEY,OTHER_MODE_MASK,SKIP_DOTS,UNIX_PATHS,BZ2,GZ,NONE,PHAR,TAR,ZIP,COMPRESSED,PHP,PHPS,MD5,OPENSSL,OPENSSL_SHA256,OPENSSL_SHA512,SHA1,SHA256,SHA512
SplTempFileObject: DROP_NEW_LINE,READ_AHEAD,SKIP_EMPTY,READ_CSV
DateTimeImmutable: ATOM,COOKIE,ISO8601,ISO8601_EXPANDED,RFC822,RFC850,RFC1036,RFC1123,RFC7231,RFC2822,RFC3339,RFC3339_EXTENDED,RSS,W3C
Class [ <internal:SPL> <iterateable> class RecursiveArrayIterator extends ArrayIterator implements Countable, Serializable, ArrayAccess, Iterator, Traversable, SeekableIterator, RecursiveIterator ] {

  - Constants [3] {
    Constant [ public int STD_PROP_LIST ] { 1 }
    Constant [ public int ARRAY_AS_PROPS ] { 2 }
    Constant [ public int CHILD_ARRAYS_ONLY ] { 4 }
  }

  - Static properties [0] {
  }

  - Static methods [0] {
  }

  - Properties [0] {
  }

  - Methods [29] {
    Method [ <internal:SPL, prototype RecursiveIterator> public method hasChildren ] {

      - Parameters [0] {
      }
      - Tentative return [ bool ]
    }

    Method [ <internal:SPL, prototype RecursiveIterator> public method getChildren ] {

      - Parameters [0] {
      }
      - Tentative return [ ?RecursiveArrayIterator ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, ctor> public method __construct ] {

      - Parameters [2] {
        Parameter #0 [ <optional> object|array $array = [] ]
        Parameter #1 [ <optional> int $flags = 0 ]
      }
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype ArrayAccess> public method offsetExists ] {

      - Parameters [1] {
        Parameter #0 [ <required> mixed $key ]
      }
      - Tentative return [ bool ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype ArrayAccess> public method offsetGet ] {

      - Parameters [1] {
        Parameter #0 [ <required> mixed $key ]
      }
      - Tentative return [ mixed ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype ArrayAccess> public method offsetSet ] {

      - Parameters [2] {
        Parameter #0 [ <required> mixed $key ]
        Parameter #1 [ <required> mixed $value ]
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype ArrayAccess> public method offsetUnset ] {

      - Parameters [1] {
        Parameter #0 [ <required> mixed $key ]
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method append ] {

      - Parameters [1] {
        Parameter #0 [ <required> mixed $value ]
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method getArrayCopy ] {

      - Parameters [0] {
      }
      - Tentative return [ array ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Countable> public method count ] {

      - Parameters [0] {
      }
      - Tentative return [ int ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method getFlags ] {

      - Parameters [0] {
      }
      - Tentative return [ int ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method setFlags ] {

      - Parameters [1] {
        Parameter #0 [ <required> int $flags ]
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method asort ] {

      - Parameters [1] {
        Parameter #0 [ <optional> int $flags = SORT_REGULAR ]
      }
      - Tentative return [ true ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method ksort ] {

      - Parameters [1] {
        Parameter #0 [ <optional> int $flags = SORT_REGULAR ]
      }
      - Tentative return [ true ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method uasort ] {

      - Parameters [1] {
        Parameter #0 [ <required> callable $callback ]
      }
      - Tentative return [ true ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method uksort ] {

      - Parameters [1] {
        Parameter #0 [ <required> callable $callback ]
      }
      - Tentative return [ true ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method natsort ] {

      - Parameters [0] {
      }
      - Tentative return [ true ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method natcasesort ] {

      - Parameters [0] {
      }
      - Tentative return [ true ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Serializable> public method unserialize ] {

      - Parameters [1] {
        Parameter #0 [ <required> string $data ]
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Serializable> public method serialize ] {

      - Parameters [0] {
      }
      - Tentative return [ string ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method __serialize ] {

      - Parameters [0] {
      }
      - Tentative return [ array ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method __unserialize ] {

      - Parameters [1] {
        Parameter #0 [ <required> array $data ]
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Iterator> public method rewind ] {

      - Parameters [0] {
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Iterator> public method current ] {

      - Parameters [0] {
      }
      - Tentative return [ mixed ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Iterator> public method key ] {

      - Parameters [0] {
      }
      - Tentative return [ string|int|null ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Iterator> public method next ] {

      - Parameters [0] {
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype Iterator> public method valid ] {

      - Parameters [0] {
      }
      - Tentative return [ bool ]
    }

    Method [ <internal:SPL, inherits ArrayIterator, prototype SeekableIterator> public method seek ] {

      - Parameters [1] {
        Parameter #0 [ <required> int $offset ]
      }
      - Tentative return [ void ]
    }

    Method [ <internal:SPL, inherits ArrayIterator> public method __debugInfo ] {

      - Parameters [0] {
      }
      - Tentative return [ array ]
    }
  }
}
bool(false)
