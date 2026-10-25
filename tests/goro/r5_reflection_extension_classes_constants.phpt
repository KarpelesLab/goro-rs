--TEST--
ReflectionExtension lists the classes and constants the extension's stubs declare
--FILE--
<?php
foreach (['random', 'date', 'json', 'xmlwriter', 'SPL'] as $e) {
    $r = new ReflectionExtension($e);
    $c = $r->getClassNames(); sort($c);
    echo $e, ': ', implode(',', $c), "\n";
    $k = array_keys($r->getConstants()); sort($k);
    echo '  ', count($k), ' constants, first: ', implode(',', array_slice($k, 0, 3)), "\n";
}
--EXPECT--
random: Random\BrokenRandomEngineError,Random\CryptoSafeEngine,Random\Engine,Random\Engine\Mt19937,Random\Engine\PcgOneseq128XslRr64,Random\Engine\Secure,Random\Engine\Xoshiro256StarStar,Random\IntervalBoundary,Random\RandomError,Random\RandomException,Random\Randomizer
  2 constants, first: MT_RAND_MT19937,MT_RAND_PHP
date: DateError,DateException,DateInterval,DateInvalidOperationException,DateInvalidTimeZoneException,DateMalformedIntervalStringException,DateMalformedPeriodStringException,DateMalformedStringException,DateObjectError,DatePeriod,DateRangeError,DateTime,DateTimeImmutable,DateTimeInterface,DateTimeZone
  17 constants, first: DATE_ATOM,DATE_COOKIE,DATE_ISO8601
json: JsonException,JsonSerializable
  29 constants, first: JSON_BIGINT_AS_STRING,JSON_ERROR_CTRL_CHAR,JSON_ERROR_DEPTH
xmlwriter: XMLWriter
  0 constants, first: 
SPL: AppendIterator,ArrayIterator,ArrayObject,BadFunctionCallException,BadMethodCallException,CachingIterator,CallbackFilterIterator,DirectoryIterator,DomainException,EmptyIterator,FilesystemIterator,FilterIterator,GlobIterator,InfiniteIterator,InvalidArgumentException,IteratorIterator,LengthException,LimitIterator,LogicException,MultipleIterator,NoRewindIterator,OutOfBoundsException,OutOfRangeException,OuterIterator,OverflowException,ParentIterator,RangeException,RecursiveArrayIterator,RecursiveCachingIterator,RecursiveCallbackFilterIterator,RecursiveDirectoryIterator,RecursiveFilterIterator,RecursiveIterator,RecursiveIteratorIterator,RecursiveRegexIterator,RecursiveTreeIterator,RegexIterator,RuntimeException,SeekableIterator,SplDoublyLinkedList,SplFileInfo,SplFileObject,SplFixedArray,SplHeap,SplMaxHeap,SplMinHeap,SplObjectStorage,SplObserver,SplPriorityQueue,SplQueue,SplStack,SplSubject,SplTempFileObject,UnderflowException,UnexpectedValueException
  0 constants, first: 
