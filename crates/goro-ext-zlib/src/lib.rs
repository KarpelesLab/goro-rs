use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

mod codec;

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"zlib");
    vm.register_function(b"gzcompress", codec::gzcompress);
    vm.register_function(b"gzuncompress", codec::gzuncompress);
    vm.register_function(b"gzdeflate", codec::gzdeflate);
    vm.register_function(b"gzinflate", codec::gzinflate);
    vm.register_function(b"gzencode", codec::gzencode);
    vm.register_function(b"gzdecode", codec::gzdecode);
    vm.register_function(b"zlib_encode", codec::zlib_encode);
    vm.register_function(b"zlib_decode", codec::zlib_decode);
    vm.register_function(b"zlib_get_coding_type", zlib_get_coding_type);
    vm.register_function(b"ob_gzhandler", ob_gzhandler_fn);

    // gzopen() & co. and the gz* aliases of the stream functions are served
    // by the stream layer of ext/standard.
    goro_ext_standard::register_zlib_stream_functions(vm);

    // deflate_init/add, inflate_init/add
    vm.register_function(b"deflate_init", codec::deflate_init);
    vm.register_function(b"deflate_add", codec::deflate_add);
    vm.register_function(b"inflate_init", codec::inflate_init);
    vm.register_function(b"inflate_add", codec::inflate_add);
    vm.register_function(b"inflate_get_read_len", codec::inflate_get_read_len);
    vm.register_function(b"inflate_get_status", codec::inflate_get_status);

    vm.builtin_param_names.insert(
        b"gzcompress".to_vec(),
        vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzuncompress".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzdeflate".to_vec(),
        vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzinflate".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzencode".to_vec(),
        vec![b"data".to_vec(), b"level".to_vec(), b"encoding".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzdecode".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"zlib_encode".to_vec(),
        vec![b"data".to_vec(), b"encoding".to_vec(), b"level".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"zlib_decode".to_vec(),
        vec![b"data".to_vec(), b"max_length".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"zlib_get_coding_type".to_vec(), vec![]);

    vm.builtin_param_names.insert(
        b"gzopen".to_vec(),
        vec![
            b"filename".to_vec(),
            b"mode".to_vec(),
            b"use_include_path".to_vec(),
        ],
    );
    vm.builtin_param_names
        .insert(b"gzclose".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(
        b"gzread".to_vec(),
        vec![b"stream".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzwrite".to_vec(),
        vec![b"stream".to_vec(), b"data".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzputs".to_vec(),
        vec![b"stream".to_vec(), b"data".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"gzgets".to_vec(),
        vec![b"stream".to_vec(), b"length".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"gzgetc".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names
        .insert(b"gzeof".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names
        .insert(b"gzrewind".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(
        b"gzseek".to_vec(),
        vec![b"stream".to_vec(), b"offset".to_vec(), b"whence".to_vec()],
    );
    vm.builtin_param_names
        .insert(b"gztell".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names
        .insert(b"gzpassthru".to_vec(), vec![b"stream".to_vec()]);
    vm.builtin_param_names.insert(
        b"gzfile".to_vec(),
        vec![b"filename".to_vec(), b"use_include_path".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"readgzfile".to_vec(),
        vec![b"filename".to_vec(), b"use_include_path".to_vec()],
    );

    vm.builtin_param_names.insert(
        b"deflate_init".to_vec(),
        vec![b"encoding".to_vec(), b"options".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"deflate_add".to_vec(),
        vec![
            b"context".to_vec(),
            b"data".to_vec(),
            b"flush_mode".to_vec(),
        ],
    );
    vm.builtin_param_names.insert(
        b"inflate_init".to_vec(),
        vec![b"encoding".to_vec(), b"options".to_vec()],
    );
    vm.builtin_param_names.insert(
        b"inflate_add".to_vec(),
        vec![
            b"context".to_vec(),
            b"data".to_vec(),
            b"flush_mode".to_vec(),
        ],
    );
    vm.builtin_param_names
        .insert(b"inflate_get_read_len".to_vec(), vec![b"context".to_vec()]);
    vm.builtin_param_names
        .insert(b"inflate_get_status".to_vec(), vec![b"context".to_vec()]);

    vm.constants
        .insert(b"ZLIB_ENCODING_RAW".to_vec(), Value::Long(-15));
    vm.constants
        .insert(b"ZLIB_ENCODING_DEFLATE".to_vec(), Value::Long(15));
    vm.constants
        .insert(b"ZLIB_ENCODING_GZIP".to_vec(), Value::Long(31));
    vm.constants.insert(b"FORCE_GZIP".to_vec(), Value::Long(31));
    vm.constants
        .insert(b"FORCE_DEFLATE".to_vec(), Value::Long(15));
    vm.constants
        .insert(b"ZLIB_NO_FLUSH".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"ZLIB_PARTIAL_FLUSH".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"ZLIB_SYNC_FLUSH".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"ZLIB_FULL_FLUSH".to_vec(), Value::Long(3));
    vm.constants.insert(b"ZLIB_FINISH".to_vec(), Value::Long(4));
    vm.constants.insert(b"ZLIB_BLOCK".to_vec(), Value::Long(5));
    vm.constants.insert(b"ZLIB_OK".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"ZLIB_STREAM_END".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"ZLIB_NEED_DICT".to_vec(), Value::Long(2));
    vm.constants.insert(b"ZLIB_ERRNO".to_vec(), Value::Long(-1));
    vm.constants
        .insert(b"ZLIB_STREAM_ERROR".to_vec(), Value::Long(-2));
    vm.constants
        .insert(b"ZLIB_DATA_ERROR".to_vec(), Value::Long(-3));
    vm.constants
        .insert(b"ZLIB_MEM_ERROR".to_vec(), Value::Long(-4));
    vm.constants
        .insert(b"ZLIB_BUF_ERROR".to_vec(), Value::Long(-5));
    vm.constants
        .insert(b"ZLIB_VERSION_ERROR".to_vec(), Value::Long(-6));
    vm.constants
        .insert(b"ZLIB_FILTERED".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"ZLIB_HUFFMAN_ONLY".to_vec(), Value::Long(2));
    vm.constants.insert(b"ZLIB_RLE".to_vec(), Value::Long(3));
    vm.constants.insert(b"ZLIB_FIXED".to_vec(), Value::Long(4));
    vm.constants
        .insert(b"ZLIB_DEFAULT_STRATEGY".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"ZLIB_VERNUM".to_vec(), Value::Long(goro_zlib::ZLIB_VERNUM));
    vm.constants.insert(
        b"ZLIB_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(goro_zlib::ZLIB_VERSION.as_bytes())),
    );
    codec::register_classes(vm);
}

fn zlib_get_coding_type(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    // In CLI mode, no content encoding is active
    Ok(Value::False)
}

fn ob_gzhandler_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    // ob_gzhandler($data, $flags) - we don't actually compress, just pass through
    // since we have no way to know the Accept-Encoding header
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    Ok(Value::String(data))
}
