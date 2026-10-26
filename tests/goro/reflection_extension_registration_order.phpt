--TEST--
ReflectionExtension lists functions and classes in registration order; date INI entries
--FILE--
<?php
foreach (['json', 'ctype', 'date', 'pcre', 'spl', 'random', 'Reflection'] as $e) {
  $r = new ReflectionExtension($e);
  echo $e, " F: ", implode(",", array_keys($r->getFunctions())), "\n";
  echo $e, " C: ", implode(",", $r->getClassNames()), "\n";
}
var_dump((new ReflectionExtension('date'))->getINIEntries());
--EXPECT--
json F: json_encode,json_decode,json_validate,json_last_error,json_last_error_msg
json C: JsonSerializable,JsonException
ctype F: ctype_alnum,ctype_alpha,ctype_cntrl,ctype_digit,ctype_lower,ctype_graph,ctype_print,ctype_punct,ctype_space,ctype_upper,ctype_xdigit
ctype C: 
date F: strtotime,date,idate,gmdate,mktime,gmmktime,checkdate,strftime,gmstrftime,time,localtime,getdate,date_create,date_create_immutable,date_create_from_format,date_create_immutable_from_format,date_parse,date_parse_from_format,date_get_last_errors,date_format,date_modify,date_add,date_sub,date_timezone_get,date_timezone_set,date_offset_get,date_diff,date_time_set,date_date_set,date_isodate_set,date_timestamp_set,date_timestamp_get,timezone_open,timezone_name_get,timezone_name_from_abbr,timezone_offset_get,timezone_transitions_get,timezone_location_get,timezone_identifiers_list,timezone_abbreviations_list,timezone_version_get,date_interval_create_from_date_string,date_interval_format,date_default_timezone_set,date_default_timezone_get,date_sunrise,date_sunset,date_sun_info
date C: DateTimeInterface,DateTime,DateTimeImmutable,DateTimeZone,DateInterval,DatePeriod,DateError,DateObjectError,DateRangeError,DateException,DateInvalidTimeZoneException,DateInvalidOperationException,DateMalformedStringException,DateMalformedIntervalStringException,DateMalformedPeriodStringException
pcre F: preg_match,preg_match_all,preg_replace,preg_filter,preg_replace_callback,preg_replace_callback_array,preg_split,preg_quote,preg_grep,preg_last_error,preg_last_error_msg
pcre C: 
spl F: class_implements,class_parents,class_uses,spl_autoload,spl_autoload_call,spl_autoload_extensions,spl_autoload_functions,spl_autoload_register,spl_autoload_unregister,spl_classes,spl_object_hash,spl_object_id,iterator_apply,iterator_count,iterator_to_array
spl C: LogicException,BadFunctionCallException,BadMethodCallException,DomainException,InvalidArgumentException,LengthException,OutOfRangeException,RuntimeException,OutOfBoundsException,OverflowException,RangeException,UnderflowException,UnexpectedValueException,RecursiveIterator,OuterIterator,RecursiveIteratorIterator,IteratorIterator,FilterIterator,RecursiveFilterIterator,CallbackFilterIterator,RecursiveCallbackFilterIterator,ParentIterator,SeekableIterator,LimitIterator,CachingIterator,RecursiveCachingIterator,NoRewindIterator,AppendIterator,InfiniteIterator,RegexIterator,RecursiveRegexIterator,EmptyIterator,RecursiveTreeIterator,ArrayObject,ArrayIterator,RecursiveArrayIterator,SplFileInfo,DirectoryIterator,FilesystemIterator,RecursiveDirectoryIterator,GlobIterator,SplFileObject,SplTempFileObject,SplDoublyLinkedList,SplQueue,SplStack,SplHeap,SplMinHeap,SplMaxHeap,SplPriorityQueue,SplFixedArray,SplObserver,SplSubject,SplObjectStorage,MultipleIterator
random F: lcg_value,mt_srand,srand,rand,mt_rand,mt_getrandmax,getrandmax,random_bytes,random_int
random C: Random\Engine,Random\CryptoSafeEngine,Random\RandomError,Random\BrokenRandomEngineError,Random\RandomException,Random\Engine\Mt19937,Random\Engine\PcgOneseq128XslRr64,Random\Engine\Xoshiro256StarStar,Random\Engine\Secure,Random\Randomizer,Random\IntervalBoundary
Reflection F: 
Reflection C: ReflectionException,Reflection,Reflector,ReflectionFunctionAbstract,ReflectionFunction,ReflectionGenerator,ReflectionParameter,ReflectionType,ReflectionNamedType,ReflectionUnionType,ReflectionIntersectionType,ReflectionMethod,ReflectionClass,ReflectionObject,ReflectionProperty,ReflectionClassConstant,ReflectionExtension,ReflectionZendExtension,ReflectionReference,ReflectionAttribute,ReflectionEnum,ReflectionEnumUnitCase,ReflectionEnumBackedCase,ReflectionFiber,ReflectionConstant,PropertyHookType
array(5) {
  ["date.timezone"]=>
  string(3) "UTC"
  ["date.default_latitude"]=>
  string(7) "31.7667"
  ["date.default_longitude"]=>
  string(7) "35.2333"
  ["date.sunset_zenith"]=>
  string(9) "90.833333"
  ["date.sunrise_zenith"]=>
  string(9) "90.833333"
}
