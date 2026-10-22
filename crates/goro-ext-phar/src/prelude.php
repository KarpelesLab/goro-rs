<?php
// The classes of php-src ext/phar (phar_object.stub.php), declared from PHP
// source when the extension is registered and then made internal classes
// (see lib.rs). The methods hand their arguments over to the native
// implementation (object.rs), which parses them like the C methods.

class PharException extends Exception
{
}

class Phar extends RecursiveDirectoryIterator implements Countable, ArrayAccess
{
    const int BZ2 = 8192;
    const int GZ = 4096;
    const int NONE = 0;
    const int PHAR = 1;
    const int TAR = 2;
    const int ZIP = 3;
    const int COMPRESSED = 61440;
    const int PHP = 0;
    const int PHPS = 1;
    const int MD5 = 1;
    const int OPENSSL = 16;
    const int OPENSSL_SHA256 = 17;
    const int OPENSSL_SHA512 = 18;
    const int SHA1 = 2;
    const int SHA256 = 3;
    const int SHA512 = 4;

    public function __construct(...$args)
    {
        $r = \__phar_m('Phar::__construct', $this, $args);
        if ($r !== null) {
            parent::__construct($r[0], $r[1]);
            \__phar_m('Phar::__construct_done', $this, []);
        }
    }

    public function __destruct()
    {
        \__phar_m('Phar::__destruct', $this, []);
    }

    public function addEmptyDir(...$args)
    {
        return \__phar_m('Phar::addEmptyDir', $this, $args);
    }

    public function addFile(...$args)
    {
        return \__phar_m('Phar::addFile', $this, $args);
    }

    public function addFromString(...$args)
    {
        return \__phar_m('Phar::addFromString', $this, $args);
    }

    public function buildFromDirectory(...$args)
    {
        $r = \__phar_m('Phar::buildFromDirectory', $this, $args);
        if ($r === null) {
            return null;
        }
        $it = new \RecursiveIteratorIterator(new \RecursiveDirectoryIterator($r[0], \FilesystemIterator::SKIP_DOTS | \FilesystemIterator::UNIX_PATHS));
        if ($r[1] !== '') {
            $it = new \RegexIterator($it, $r[1]);
        }
        $r[1] = $r[0];
        \__phar_build_begin($this, \get_class($it), $r[1]);
        try {
            while ($it instanceof \IteratorAggregate) {
                $it = $it->getIterator();
            }
            for ($it->rewind(); $it->valid(); $it->next()) {
                \__phar_build_item($it->current(), $it);
            }
        } catch (\Throwable $e) {
            \__phar_build_end(false);
            throw $e;
        }
        return \__phar_build_end(true);
    }

    public function buildFromIterator(...$args)
    {
        $r = \__phar_m('Phar::buildFromIterator', $this, $args);
        if ($r === null) {
            return null;
        }
        $it = $r[0];
        \__phar_build_begin($this, \get_class($it), $r[1]);
        try {
            while ($it instanceof \IteratorAggregate) {
                $it = $it->getIterator();
            }
            for ($it->rewind(); $it->valid(); $it->next()) {
                \__phar_build_item($it->current(), $it);
            }
        } catch (\Throwable $e) {
            \__phar_build_end(false);
            throw $e;
        }
        return \__phar_build_end(true);
    }

    public function compressFiles(...$args)
    {
        return \__phar_m('Phar::compressFiles', $this, $args);
    }

    public function decompressFiles(...$args)
    {
        return \__phar_m('Phar::decompressFiles', $this, $args);
    }

    public function compress(...$args)
    {
        $r = \__phar_m('Phar::compress', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function decompress(...$args)
    {
        $r = \__phar_m('Phar::decompress', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function convertToExecutable(...$args)
    {
        $r = \__phar_m('Phar::convertToExecutable', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function convertToData(...$args)
    {
        $r = \__phar_m('Phar::convertToData', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function copy(...$args)
    {
        return \__phar_m('Phar::copy', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function count(...$args)
    {
        return \__phar_m('Phar::count', $this, $args);
    }

    public function delete(...$args)
    {
        return \__phar_m('Phar::delete', $this, $args);
    }

    public function delMetadata(...$args)
    {
        return \__phar_m('Phar::delMetadata', $this, $args);
    }

    public function extractTo(...$args)
    {
        return \__phar_m('Phar::extractTo', $this, $args);
    }

    public function getAlias(...$args)
    {
        return \__phar_m('Phar::getAlias', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function getPath(...$args)
    {
        return \__phar_m('Phar::getPath', $this, $args);
    }

    public function getMetadata(...$args)
    {
        return \__phar_m('Phar::getMetadata', $this, $args);
    }

    public function getModified(...$args)
    {
        return \__phar_m('Phar::getModified', $this, $args);
    }

    public function getSignature(...$args)
    {
        return \__phar_m('Phar::getSignature', $this, $args);
    }

    public function getStub(...$args)
    {
        return \__phar_m('Phar::getStub', $this, $args);
    }

    public function getVersion(...$args)
    {
        return \__phar_m('Phar::getVersion', $this, $args);
    }

    public function hasMetadata(...$args)
    {
        return \__phar_m('Phar::hasMetadata', $this, $args);
    }

    public function isBuffering(...$args)
    {
        return \__phar_m('Phar::isBuffering', $this, $args);
    }

    public function isCompressed(...$args)
    {
        return \__phar_m('Phar::isCompressed', $this, $args);
    }

    public function isFileFormat(...$args)
    {
        return \__phar_m('Phar::isFileFormat', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function isWritable(...$args)
    {
        return \__phar_m('Phar::isWritable', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function offsetExists(...$args)
    {
        return \__phar_m('Phar::offsetExists', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function offsetGet(...$args)
    {
        $r = \__phar_m('Phar::offsetGet', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    #[\ReturnTypeWillChange]
    public function offsetSet(...$args)
    {
        return \__phar_m('Phar::offsetSet', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function offsetUnset(...$args)
    {
        return \__phar_m('Phar::offsetUnset', $this, $args);
    }

    public function setAlias(...$args)
    {
        return \__phar_m('Phar::setAlias', $this, $args);
    }

    public function setDefaultStub(...$args)
    {
        return \__phar_m('Phar::setDefaultStub', $this, $args);
    }

    public function setMetadata(...$args)
    {
        return \__phar_m('Phar::setMetadata', $this, $args);
    }

    public function setSignatureAlgorithm(...$args)
    {
        return \__phar_m('Phar::setSignatureAlgorithm', $this, $args);
    }

    public function setStub(...$args)
    {
        return \__phar_m('Phar::setStub', $this, $args);
    }

    public function startBuffering(...$args)
    {
        return \__phar_m('Phar::startBuffering', $this, $args);
    }

    public function stopBuffering(...$args)
    {
        return \__phar_m('Phar::stopBuffering', $this, $args);
    }

    final public static function apiVersion(...$args)
    {
        return \__phar_m('Phar::apiVersion', null, $args);
    }

    final public static function canCompress(...$args)
    {
        return \__phar_m('Phar::canCompress', null, $args);
    }

    final public static function canWrite(...$args)
    {
        return \__phar_m('Phar::canWrite', null, $args);
    }

    final public static function createDefaultStub(...$args)
    {
        return \__phar_m('Phar::createDefaultStub', null, $args);
    }

    final public static function getSupportedCompression(...$args)
    {
        return \__phar_m('Phar::getSupportedCompression', null, $args);
    }

    final public static function getSupportedSignatures(...$args)
    {
        return \__phar_m('Phar::getSupportedSignatures', null, $args);
    }

    final public static function interceptFileFuncs(...$args)
    {
        return \__phar_m('Phar::interceptFileFuncs', null, $args);
    }

    final public static function isValidPharFilename(...$args)
    {
        return \__phar_m('Phar::isValidPharFilename', null, $args);
    }

    final public static function loadPhar(...$args)
    {
        return \__phar_m('Phar::loadPhar', null, $args);
    }

    final public static function mapPhar(...$args)
    {
        return \__phar_m('Phar::mapPhar', null, $args);
    }

    final public static function running(...$args)
    {
        return \__phar_m('Phar::running', null, $args);
    }

    final public static function mount(...$args)
    {
        return \__phar_m('Phar::mount', null, $args);
    }

    final public static function mungServer(...$args)
    {
        return \__phar_m('Phar::mungServer', null, $args);
    }

    final public static function unlinkArchive(...$args)
    {
        return \__phar_m('Phar::unlinkArchive', null, $args);
    }

    final public static function webPhar(...$args)
    {
        return \__phar_m('Phar::webPhar', null, $args);
    }

}

class PharData extends RecursiveDirectoryIterator implements Countable, ArrayAccess
{
    public function __construct(...$args)
    {
        $r = \__phar_m('PharData::__construct', $this, $args);
        if ($r !== null) {
            parent::__construct($r[0], $r[1]);
            \__phar_m('PharData::__construct_done', $this, []);
        }
    }

    public function __destruct()
    {
        \__phar_m('PharData::__destruct', $this, []);
    }

    public function addEmptyDir(...$args)
    {
        return \__phar_m('PharData::addEmptyDir', $this, $args);
    }

    public function addFile(...$args)
    {
        return \__phar_m('PharData::addFile', $this, $args);
    }

    public function addFromString(...$args)
    {
        return \__phar_m('PharData::addFromString', $this, $args);
    }

    public function buildFromDirectory(...$args)
    {
        $r = \__phar_m('PharData::buildFromDirectory', $this, $args);
        if ($r === null) {
            return null;
        }
        $it = new \RecursiveIteratorIterator(new \RecursiveDirectoryIterator($r[0], \FilesystemIterator::SKIP_DOTS | \FilesystemIterator::UNIX_PATHS));
        if ($r[1] !== '') {
            $it = new \RegexIterator($it, $r[1]);
        }
        $r[1] = $r[0];
        \__phar_build_begin($this, \get_class($it), $r[1]);
        try {
            while ($it instanceof \IteratorAggregate) {
                $it = $it->getIterator();
            }
            for ($it->rewind(); $it->valid(); $it->next()) {
                \__phar_build_item($it->current(), $it);
            }
        } catch (\Throwable $e) {
            \__phar_build_end(false);
            throw $e;
        }
        return \__phar_build_end(true);
    }

    public function buildFromIterator(...$args)
    {
        $r = \__phar_m('PharData::buildFromIterator', $this, $args);
        if ($r === null) {
            return null;
        }
        $it = $r[0];
        \__phar_build_begin($this, \get_class($it), $r[1]);
        try {
            while ($it instanceof \IteratorAggregate) {
                $it = $it->getIterator();
            }
            for ($it->rewind(); $it->valid(); $it->next()) {
                \__phar_build_item($it->current(), $it);
            }
        } catch (\Throwable $e) {
            \__phar_build_end(false);
            throw $e;
        }
        return \__phar_build_end(true);
    }

    public function compressFiles(...$args)
    {
        return \__phar_m('PharData::compressFiles', $this, $args);
    }

    public function decompressFiles(...$args)
    {
        return \__phar_m('PharData::decompressFiles', $this, $args);
    }

    public function compress(...$args)
    {
        $r = \__phar_m('PharData::compress', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function decompress(...$args)
    {
        $r = \__phar_m('PharData::decompress', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function convertToExecutable(...$args)
    {
        $r = \__phar_m('PharData::convertToExecutable', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function convertToData(...$args)
    {
        $r = \__phar_m('PharData::convertToData', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    public function copy(...$args)
    {
        return \__phar_m('PharData::copy', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function count(...$args)
    {
        return \__phar_m('PharData::count', $this, $args);
    }

    public function delete(...$args)
    {
        return \__phar_m('PharData::delete', $this, $args);
    }

    public function delMetadata(...$args)
    {
        return \__phar_m('PharData::delMetadata', $this, $args);
    }

    public function extractTo(...$args)
    {
        return \__phar_m('PharData::extractTo', $this, $args);
    }

    public function getAlias(...$args)
    {
        return \__phar_m('PharData::getAlias', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function getPath(...$args)
    {
        return \__phar_m('PharData::getPath', $this, $args);
    }

    public function getMetadata(...$args)
    {
        return \__phar_m('PharData::getMetadata', $this, $args);
    }

    public function getModified(...$args)
    {
        return \__phar_m('PharData::getModified', $this, $args);
    }

    public function getSignature(...$args)
    {
        return \__phar_m('PharData::getSignature', $this, $args);
    }

    public function getStub(...$args)
    {
        return \__phar_m('PharData::getStub', $this, $args);
    }

    public function getVersion(...$args)
    {
        return \__phar_m('PharData::getVersion', $this, $args);
    }

    public function hasMetadata(...$args)
    {
        return \__phar_m('PharData::hasMetadata', $this, $args);
    }

    public function isBuffering(...$args)
    {
        return \__phar_m('PharData::isBuffering', $this, $args);
    }

    public function isCompressed(...$args)
    {
        return \__phar_m('PharData::isCompressed', $this, $args);
    }

    public function isFileFormat(...$args)
    {
        return \__phar_m('PharData::isFileFormat', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function isWritable(...$args)
    {
        return \__phar_m('PharData::isWritable', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function offsetExists(...$args)
    {
        return \__phar_m('PharData::offsetExists', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function offsetGet(...$args)
    {
        $r = \__phar_m('PharData::offsetGet', $this, $args);
        if ($r === null) {
            return null;
        }
        $c = $r[0];
        return new $c($r[1]);
    }

    #[\ReturnTypeWillChange]
    public function offsetSet(...$args)
    {
        return \__phar_m('PharData::offsetSet', $this, $args);
    }

    #[\ReturnTypeWillChange]
    public function offsetUnset(...$args)
    {
        return \__phar_m('PharData::offsetUnset', $this, $args);
    }

    public function setAlias(...$args)
    {
        return \__phar_m('PharData::setAlias', $this, $args);
    }

    public function setDefaultStub(...$args)
    {
        return \__phar_m('PharData::setDefaultStub', $this, $args);
    }

    public function setMetadata(...$args)
    {
        return \__phar_m('PharData::setMetadata', $this, $args);
    }

    public function setSignatureAlgorithm(...$args)
    {
        return \__phar_m('PharData::setSignatureAlgorithm', $this, $args);
    }

    public function setStub(...$args)
    {
        return \__phar_m('PharData::setStub', $this, $args);
    }

    public function startBuffering(...$args)
    {
        return \__phar_m('PharData::startBuffering', $this, $args);
    }

    public function stopBuffering(...$args)
    {
        return \__phar_m('PharData::stopBuffering', $this, $args);
    }

    final public static function apiVersion(...$args)
    {
        return \__phar_m('PharData::apiVersion', null, $args);
    }

    final public static function canCompress(...$args)
    {
        return \__phar_m('PharData::canCompress', null, $args);
    }

    final public static function canWrite(...$args)
    {
        return \__phar_m('PharData::canWrite', null, $args);
    }

    final public static function createDefaultStub(...$args)
    {
        return \__phar_m('PharData::createDefaultStub', null, $args);
    }

    final public static function getSupportedCompression(...$args)
    {
        return \__phar_m('PharData::getSupportedCompression', null, $args);
    }

    final public static function getSupportedSignatures(...$args)
    {
        return \__phar_m('PharData::getSupportedSignatures', null, $args);
    }

    final public static function interceptFileFuncs(...$args)
    {
        return \__phar_m('PharData::interceptFileFuncs', null, $args);
    }

    final public static function isValidPharFilename(...$args)
    {
        return \__phar_m('PharData::isValidPharFilename', null, $args);
    }

    final public static function loadPhar(...$args)
    {
        return \__phar_m('PharData::loadPhar', null, $args);
    }

    final public static function mapPhar(...$args)
    {
        return \__phar_m('PharData::mapPhar', null, $args);
    }

    final public static function running(...$args)
    {
        return \__phar_m('PharData::running', null, $args);
    }

    final public static function mount(...$args)
    {
        return \__phar_m('PharData::mount', null, $args);
    }

    final public static function mungServer(...$args)
    {
        return \__phar_m('PharData::mungServer', null, $args);
    }

    final public static function unlinkArchive(...$args)
    {
        return \__phar_m('PharData::unlinkArchive', null, $args);
    }

    final public static function webPhar(...$args)
    {
        return \__phar_m('PharData::webPhar', null, $args);
    }

}

class PharFileInfo extends SplFileInfo
{
    public function __construct(...$args)
    {
        $r = \__phar_m('PharFileInfo::__construct', $this, $args);
        if ($r !== null) {
            parent::__construct($r);
        }
    }

    public function __destruct()
    {
        \__phar_m('PharFileInfo::__destruct', $this, []);
    }

    public function chmod(...$args)
    {
        return \__phar_m('PharFileInfo::chmod', $this, $args);
    }

    public function compress(...$args)
    {
        return \__phar_m('PharFileInfo::compress', $this, $args);
    }

    public function decompress(...$args)
    {
        return \__phar_m('PharFileInfo::decompress', $this, $args);
    }

    public function delMetadata(...$args)
    {
        return \__phar_m('PharFileInfo::delMetadata', $this, $args);
    }

    public function getCompressedSize(...$args)
    {
        return \__phar_m('PharFileInfo::getCompressedSize', $this, $args);
    }

    public function getCRC32(...$args)
    {
        return \__phar_m('PharFileInfo::getCRC32', $this, $args);
    }

    public function getContent(...$args)
    {
        return \__phar_m('PharFileInfo::getContent', $this, $args);
    }

    public function getMetadata(...$args)
    {
        return \__phar_m('PharFileInfo::getMetadata', $this, $args);
    }

    public function getPharFlags(...$args)
    {
        return \__phar_m('PharFileInfo::getPharFlags', $this, $args);
    }

    public function hasMetadata(...$args)
    {
        return \__phar_m('PharFileInfo::hasMetadata', $this, $args);
    }

    public function isCompressed(...$args)
    {
        return \__phar_m('PharFileInfo::isCompressed', $this, $args);
    }

    public function isCRCChecked(...$args)
    {
        return \__phar_m('PharFileInfo::isCRCChecked', $this, $args);
    }

    public function setMetadata(...$args)
    {
        return \__phar_m('PharFileInfo::setMetadata', $this, $args);
    }

}
