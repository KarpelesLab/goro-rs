//! Tables shared by the JPEG encoder and decoder (jutils.c, jcparam.c,
//! jstdhuff.c).

/// jpeg_natural_order: zigzag index -> natural (row-major) index, with
/// 16 extra entries so that corrupt run lengths cannot index past it.
pub const NATURAL_ORDER: [usize; 80] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63, 63,
];

/// std_luminance_quant_tbl (natural order)
pub const STD_LUMINANCE_QUANT: [u32; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

/// std_chrominance_quant_tbl (natural order)
pub const STD_CHROMINANCE_QUANT: [u32; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// A Huffman table as stored in a DHT marker: bits[1..=16] are the code
/// counts per length, vals the symbols.
#[derive(Clone, Debug)]
pub struct HuffSpec {
    pub bits: [u8; 17],
    pub vals: Vec<u8>,
}

fn spec(bits: &[u8; 16], vals: &[u8]) -> HuffSpec {
    let mut b = [0u8; 17];
    b[1..].copy_from_slice(bits);
    HuffSpec {
        bits: b,
        vals: vals.to_vec(),
    }
}

const DC_LUM_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHR_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUM_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUM_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const AC_CHR_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHR_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// jpeg_std_huff_table(): (is_dc, table number 0 or 1)
pub fn std_huff(is_dc: bool, tbl: usize) -> HuffSpec {
    match (is_dc, tbl) {
        (true, 0) => spec(&DC_LUM_BITS, &DC_VALS),
        (true, _) => spec(&DC_CHR_BITS, &DC_VALS),
        (false, 0) => spec(&AC_LUM_BITS, &AC_LUM_VALS),
        (false, _) => spec(&AC_CHR_BITS, &AC_CHR_VALS),
    }
}

// jfdctint.c / jidctint.c constants (CONST_BITS = 13)
pub const CONST_BITS: i32 = 13;
pub const PASS1_BITS: i32 = 2;
pub const FIX_0_298631336: i64 = 2446;
pub const FIX_0_390180644: i64 = 3196;
pub const FIX_0_541196100: i64 = 4433;
pub const FIX_0_765366865: i64 = 6270;
pub const FIX_0_899976223: i64 = 7373;
pub const FIX_1_175875602: i64 = 9633;
pub const FIX_1_501321110: i64 = 12299;
pub const FIX_1_847759065: i64 = 15137;
pub const FIX_1_961570560: i64 = 16069;
pub const FIX_2_053119869: i64 = 16819;
pub const FIX_2_562915447: i64 = 20995;
pub const FIX_3_072711026: i64 = 25172;

/// DESCALE(): round and shift right
#[inline]
pub fn descale(x: i64, n: i32) -> i64 {
    (x + (1i64 << (n - 1))) >> n
}

/// The libjpeg error/warning message texts (jerror.h) used here
pub mod msg {
    pub const INPUT_EMPTY: &str = "Empty input file";
    pub const NO_IMAGE: &str = "JPEG datastream contains no image";
    pub const JPEG_EOF: &str = "Premature end of JPEG file";
    pub const HIT_MARKER: &str = "Corrupt JPEG data: premature end of data segment";
    pub const HUFF_BAD_CODE: &str = "Corrupt JPEG data: bad Huffman code";
    pub const BAD_HUFF_TABLE: &str = "Bogus Huffman table definition";
    pub const BAD_LENGTH: &str = "Bogus marker length";
    pub const BAD_MCU_SIZE: &str = "Sampling factors too large for interleaved scan";
    pub const BAD_SAMPLING: &str = "Bogus sampling factors";
    pub const CONVERSION_NOTIMPL: &str = "Unsupported color conversion request";
    pub const EMPTY_IMAGE: &str = "Empty JPEG image (DNL not supported)";
    pub const EOI_EXPECTED: &str = "Didn't expect more than one scan";
    pub const SOF_DUPLICATE: &str = "Invalid JPEG file structure: two SOF markers";
    pub const SOF_NO_SOS: &str = "Invalid JPEG file structure: missing SOS marker";
    pub const SOI_DUPLICATE: &str = "Invalid JPEG file structure: two SOI markers";
    pub const SOS_NO_SOF: &str = "Invalid JPEG file structure: SOS before SOF";
    pub const NOT_SEQUENTIAL: &str = "Invalid SOS parameters for sequential JPEG";
    pub const FRACT_SAMPLE_NOTIMPL: &str = "Fractional sampling not implemented yet";
}
