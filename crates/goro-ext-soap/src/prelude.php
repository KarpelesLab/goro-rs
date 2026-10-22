<?php
// The classes of ext/soap (soap.stub.php), declared from PHP source when the
// extension is registered and then made internal (see lib.rs). The methods
// hand their arguments over to the native implementation, which parses them
// like the C methods (zend_parse_parameters()).

namespace Soap {
    final class Url
    {
    }

    final class Sdl
    {
    }
}

namespace {
    class SoapParam
    {
        public string $param_name;
        public mixed $param_data;

        public function __construct(...$args) { \__soap_method('SoapParam::__construct', $this, $args); }
    }

    class SoapHeader
    {
        public string $namespace;
        public string $name;
        public mixed $data = null;
        public bool $mustUnderstand;
        public string|int|null $actor;

        public function __construct(...$args) { \__soap_method('SoapHeader::__construct', $this, $args); }
    }

    class SoapFault extends Exception
    {
        public string $faultstring;
        public ?string $faultcode = null;
        public ?string $faultcodens = null;
        public ?string $faultactor = null;
        public mixed $detail = null;
        public ?string $_name = null;
        public mixed $headerfault = null;
        public string $lang = "";

        public function __construct(...$args) { \__soap_method('SoapFault::__construct', $this, $args); }

        public function __toString(): string { return \__soap_method('SoapFault::__toString', $this, \func_get_args()); }
    }

    class SoapVar
    {
        public int $enc_type;
        public mixed $enc_value = null;
        public ?string $enc_stype = null;
        public ?string $enc_ns = null;
        public ?string $enc_name = null;
        public ?string $enc_namens = null;

        public function __construct(...$args) { \__soap_method('SoapVar::__construct', $this, $args); }
    }

    class SoapServer
    {
        private ?SoapFault $__soap_fault = null;

        public function __construct(...$args) { \__soap_method('SoapServer::__construct', $this, $args); }

        public function fault(...$args) { return \__soap_method('SoapServer::fault', $this, $args); }

        public function addSoapHeader(...$args) { return \__soap_method('SoapServer::addSoapHeader', $this, $args); }

        public function setPersistence(...$args) { return \__soap_method('SoapServer::setPersistence', $this, $args); }

        public function setClass(...$args) { return \__soap_method('SoapServer::setClass', $this, $args); }

        public function setObject(...$args) { return \__soap_method('SoapServer::setObject', $this, $args); }

        public function getFunctions(...$args) { return \__soap_method('SoapServer::getFunctions', $this, $args); }

        public function addFunction(...$args) { return \__soap_method('SoapServer::addFunction', $this, $args); }

        public function handle(...$args) { return \__soap_method('SoapServer::handle', $this, $args); }

        public function __getLastResponse(...$args) { return \__soap_method('SoapServer::__getLastResponse', $this, $args); }
    }

    class SoapClient
    {
        private ?string $uri = null;
        private ?int $style = null;
        private ?int $use = null;
        private ?string $location = null;
        private bool $trace = false;
        private ?int $compression = null;
        private ?Soap\Sdl $sdl = null;
        private ?array $typemap = null;
        private $httpsocket = null;
        private ?Soap\Url $httpurl = null;

        private ?string $_login = null;
        private ?string $_password = null;
        private bool $_use_digest = false;
        private ?string $_digest = null;
        private ?string $_proxy_host = null;
        private ?int $_proxy_port = null;
        private ?string $_proxy_login = null;
        private ?string $_proxy_password = null;
        private bool $_exceptions = true;
        private ?string $_encoding = null;
        private ?array $_classmap = null;
        private ?int $_features = null;
        private int $_connection_timeout = 0;
        private $_stream_context = null;
        private ?string $_user_agent = null;
        private bool $_keep_alive = true;
        private ?int $_ssl_method = null;
        private int $_soap_version;
        private ?int $_use_proxy = null;
        private array $_cookies = [];
        private ?array $__default_headers = null;
        private ?SoapFault $__soap_fault = null;
        private ?string $__last_request = null;
        private ?string $__last_response = null;
        private ?string $__last_request_headers = null;
        private ?string $__last_response_headers = null;

        public function __construct(...$args) { \__soap_method('SoapClient::__construct', $this, $args); }

        public function __call($name, $args) { return \__soap_method('SoapClient::__call', $this, \func_get_args()); }

        public function __soapCall($name = null, $args = null, $options = null, $inputHeaders = null, &$outputHeaders = null)
        {
            $r = \__soap_method('SoapClient::__soapCall', $this, \func_get_args());
            if (\func_num_args() > 4) { $outputHeaders = \__soap_method('SoapClient::__soapCall', null, null); }
            return $r;
        }

        public function __getFunctions(...$args) { return \__soap_method('SoapClient::__getFunctions', $this, $args); }

        public function __getTypes(...$args) { return \__soap_method('SoapClient::__getTypes', $this, $args); }

        public function __getLastRequest(...$args) { return \__soap_method('SoapClient::__getLastRequest', $this, $args); }

        public function __getLastResponse(...$args) { return \__soap_method('SoapClient::__getLastResponse', $this, $args); }

        public function __getLastRequestHeaders(...$args) { return \__soap_method('SoapClient::__getLastRequestHeaders', $this, $args); }

        public function __getLastResponseHeaders(...$args) { return \__soap_method('SoapClient::__getLastResponseHeaders', $this, $args); }

        public function __doRequest(...$args) { return \__soap_method('SoapClient::__doRequest', $this, $args); }

        public function __setCookie(...$args) { return \__soap_method('SoapClient::__setCookie', $this, $args); }

        public function __getCookies(...$args) { return \__soap_method('SoapClient::__getCookies', $this, $args); }

        public function __setSoapHeaders(...$args) { return \__soap_method('SoapClient::__setSoapHeaders', $this, $args); }

        public function __setLocation(...$args) { return \__soap_method('SoapClient::__setLocation', $this, $args); }
    }
}
