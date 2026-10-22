//! Port of global.c (and the generated opcodes.c tables).

#![allow(dead_code)]

use crate::consts::*;

/// Upper-to-lower case map, followed by the 18-entry comparison result
/// tables used by sqlite3aLTb/aEQb/aGTb.
pub static sqlite3UpperToLower: [u8; 274] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49,
    50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 97, 98, 99, 100, 101, 102, 103,
    104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122,
    91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
    112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130,
    131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149,
    150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168,
    169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187,
    188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206,
    207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225,
    226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244,
    245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1,
    0, 1, 0, 0, 1,
];

/// sqlite3aLTb[op] etc. (indexed by an OP_Ne..OP_Ge opcode)
#[inline]
pub fn sqlite3aLTb(op: u8) -> u8 {
    sqlite3UpperToLower[256 - OP_Ne as usize + op as usize]
}
#[inline]
pub fn sqlite3aEQb(op: u8) -> u8 {
    sqlite3UpperToLower[256 + 6 - OP_Ne as usize + op as usize]
}
#[inline]
pub fn sqlite3aGTb(op: u8) -> u8 {
    sqlite3UpperToLower[256 + 12 - OP_Ne as usize + op as usize]
}

/// Character classification: 0x01 space, 0x02 alpha, 0x04 digit,
/// 0x08 hex digit, 0x20 lower case, 0x40 identifier char, 0x80 quote.
pub static sqlite3CtypeMap: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 0, 128, 0, 64, 0, 0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 0,
    0, 0, 0, 0, 0, 0, 10, 10, 10, 10, 10, 10, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 128, 0, 0, 0, 64, 128, 42, 42, 42, 42, 42, 42, 34, 34, 34, 34, 34, 34, 34, 34, 34, 34,
    34, 34, 34, 34, 34, 34, 34, 34, 34, 34, 0, 0, 0, 0, 0, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
];

#[inline]
pub fn sqlite3Isspace(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x01) != 0
}
#[inline]
pub fn sqlite3Isalnum(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x06) != 0
}
#[inline]
pub fn sqlite3Isalpha(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x02) != 0
}
#[inline]
pub fn sqlite3Isdigit(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x04) != 0
}
#[inline]
pub fn sqlite3Isxdigit(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x08) != 0
}
#[inline]
pub fn sqlite3Isquote(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x80) != 0
}
#[inline]
pub fn sqlite3Tolower(x: u8) -> u8 {
    sqlite3UpperToLower[x as usize]
}
#[inline]
pub fn sqlite3Toupper(x: u8) -> u8 {
    x & !(sqlite3CtypeMap[x as usize] & 0x20)
}
/// sqlite3IsIdChar()
#[inline]
pub fn sqlite3IsIdCharTab(x: u8) -> bool {
    (sqlite3CtypeMap[x as usize] & 0x46) != 0
}

/// Properties of each opcode (OPFLG_* bits).
pub static sqlite3OpcodeProperty: [u8; 191] = [
    0, 0, 0, 0, 16, 0, 65, 0, 129, 1, 1, 129, 131, 131, 1, 1, 3, 3, 1, 18, 1, 201, 201, 201, 201,
    73, 73, 73, 73, 201, 73, 193, 1, 65, 65, 193, 1, 1, 65, 65, 65, 65, 65, 38, 38, 65, 9, 35, 11,
    129, 1, 3, 3, 11, 11, 11, 11, 11, 11, 1, 3, 3, 3, 1, 65, 1, 0, 0, 2, 2, 8, 0, 16, 16, 16, 0,
    16, 0, 16, 16, 0, 0, 16, 16, 0, 0, 0, 2, 2, 2, 0, 0, 18, 30, 32, 64, 0, 0, 0, 16, 16, 0, 64,
    38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 64, 0, 18, 64, 64, 16, 64, 0, 0, 0, 64, 0, 64, 64, 16,
    16, 0, 0, 0, 0, 0, 64, 0, 80, 0, 64, 4, 4, 0, 64, 80, 64, 16, 0, 0, 16, 0, 0, 0, 0, 0, 16, 0,
    0, 6, 16, 0, 4, 26, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 16, 80, 64, 0, 16, 16, 2, 18, 18,
    0, 0, 0, 0, 0, 0, 0,
];

/// Opcode names, with the synopsis after a NUL as in opcodes.c
/// ("Name" or "Name\0synopsis").
pub static sqlite3OpcodeNames: [&str; 191] = [
    "Savepoint",
    "AutoCommit",
    "Transaction",
    "Checkpoint",
    "JournalMode",
    "Vacuum",
    "VFilter\0iplan=r[P3] zplan='P4'",
    "VUpdate\0data=r[P3@P2]",
    "Init\0Start at P2",
    "Goto",
    "Gosub",
    "InitCoroutine",
    "Yield",
    "MustBeInt",
    "Jump",
    "Once",
    "If",
    "IfNot",
    "IsType\0if typeof(P1.P3) in P5 goto P2",
    "Not\0r[P2]= !r[P1]",
    "IfNullRow\0if P1.nullRow then r[P3]=NULL, goto P2",
    "SeekLT\0key=r[P3@P4]",
    "SeekLE\0key=r[P3@P4]",
    "SeekGE\0key=r[P3@P4]",
    "SeekGT\0key=r[P3@P4]",
    "IfNoHope\0key=r[P3@P4]",
    "NoConflict\0key=r[P3@P4]",
    "NotFound\0key=r[P3@P4]",
    "Found\0key=r[P3@P4]",
    "SeekRowid\0intkey=r[P3]",
    "NotExists\0intkey=r[P3]",
    "Last",
    "IfSizeBetween",
    "SorterSort",
    "Sort",
    "Rewind",
    "IfEmpty\0if( empty(P1) ) goto P2",
    "SorterNext",
    "Prev",
    "Next",
    "IdxLE\0key=r[P3@P4]",
    "IdxGT\0key=r[P3@P4]",
    "IdxLT\0key=r[P3@P4]",
    "Or\0r[P3]=(r[P1] || r[P2])",
    "And\0r[P3]=(r[P1] && r[P2])",
    "IdxGE\0key=r[P3@P4]",
    "IFindKey",
    "RowSetRead\0r[P3]=rowset(P1)",
    "RowSetTest\0if r[P3] in rowset(P1) goto P2",
    "Program",
    "FkIfZero\0if fkctr[P1]==0 goto P2",
    "IsNull\0if r[P1]==NULL goto P2",
    "NotNull\0if r[P1]!=NULL goto P2",
    "Ne\0IF r[P3]!=r[P1]",
    "Eq\0IF r[P3]==r[P1]",
    "Gt\0IF r[P3]>r[P1]",
    "Le\0IF r[P3]<=r[P1]",
    "Lt\0IF r[P3]<r[P1]",
    "Ge\0IF r[P3]>=r[P1]",
    "ElseEq",
    "IfPos\0if r[P1]>0 then r[P1]-=P3, goto P2",
    "IfNotZero\0if r[P1]!=0 then r[P1]--, goto P2",
    "DecrJumpZero\0if (--r[P1])==0 goto P2",
    "IncrVacuum",
    "VNext",
    "Filter\0if key(P3@P4) not in filter(P1) goto P2",
    "PureFunc\0r[P3]=func(r[P2@NP])",
    "Function\0r[P3]=func(r[P2@NP])",
    "Return",
    "EndCoroutine",
    "HaltIfNull\0if r[P3]=null halt",
    "Halt",
    "Integer\0r[P2]=P1",
    "Int64\0r[P2]=PINT13",
    "String\0r[P2]='P4' (len=P1)",
    "BeginSubrtn\0r[P2]=NULL",
    "Null\0r[P2..P3]=NULL",
    "SoftNull\0r[P1]=NULL",
    "Blob\0r[P2]=P4 (len=P1)",
    "Variable\0r[P2]=parameter(P1)",
    "Move\0r[P2@P3]=r[P1@P3]",
    "Copy\0r[P2@P3+1]=r[P1@P3+1]",
    "SCopy\0r[P2]=r[P1]",
    "IntCopy\0r[P2]=r[P1]",
    "FkCheck",
    "ResultRow\0output=r[P1@P2]",
    "CollSeq",
    "AddImm\0r[P1]=r[P1]+P2",
    "RealAffinity",
    "Cast\0affinity(r[P1])",
    "Permutation",
    "Compare\0r[P1@P3] <-> r[P2@P3]",
    "IsTrue\0r[P2] = coalesce(r[P1]==TRUE,P3) ^ P4",
    "ZeroOrNull\0r[P2] = 0 OR NULL",
    "Offset\0r[P3] = sqlite_offset(P1)",
    "Column\0r[P3]=PX cursor P1 column P2",
    "TypeCheck\0typecheck(r[P1@P2])",
    "Affinity\0affinity(r[P1@P2])",
    "MakeRecord\0r[P3]=mkrec(r[P1@P2])",
    "Count\0r[P2]=count()",
    "ReadCookie",
    "SetCookie",
    "ReopenIdx\0root=P2 iDb=P3",
    "BitAnd\0r[P3]=r[P1]&r[P2]",
    "BitOr\0r[P3]=r[P1]|r[P2]",
    "ShiftLeft\0r[P3]=r[P2]<<r[P1]",
    "ShiftRight\0r[P3]=r[P2]>>r[P1]",
    "Add\0r[P3]=r[P1]+r[P2]",
    "Subtract\0r[P3]=r[P2]-r[P1]",
    "Multiply\0r[P3]=r[P1]*r[P2]",
    "Divide\0r[P3]=r[P2]/r[P1]",
    "Remainder\0r[P3]=r[P2]%r[P1]",
    "Concat\0r[P3]=r[P2]+r[P1]",
    "OpenRead\0root=P2 iDb=P3",
    "OpenWrite\0root=P2 iDb=P3",
    "BitNot\0r[P2]= ~r[P1]",
    "OpenDup",
    "OpenAutoindex\0nColumn=P2",
    "String8\0r[P2]='P4'",
    "OpenEphemeral\0nColumn=P2",
    "SorterOpen",
    "SequenceTest\0if( cursor[P1].ctr++ ) pc = P2",
    "OpenPseudo\0P3 columns in r[P2]",
    "Close",
    "ColumnsUsed\0Cursor P1 uses columns PHEX23",
    "SeekScan\0Scan-ahead up to P1 rows",
    "SeekHit\0set P2<=seekHit<=P3",
    "Sequence\0r[P2]=cursor[P1].ctr++",
    "NewRowid\0r[P2]=rowid",
    "Insert\0intkey=r[P3] data=r[P2]",
    "RowCell",
    "Delete",
    "ResetCount",
    "SorterCompare\0if key(P1)!=trim(r[P3],P4) goto P2",
    "SorterData\0r[P2]=data",
    "RowData\0r[P2]=data",
    "Rowid\0r[P2]=PX rowid of P1",
    "NullRow",
    "SeekEnd",
    "IdxInsert\0key=r[P2]",
    "SorterInsert\0key=r[P2]",
    "IdxDelete\0key=r[P2@P5]",
    "DeferredSeek\0Move P3 to P1.rowid if needed",
    "IdxRowid\0r[P2]=rowid",
    "FinishSeek",
    "Destroy",
    "Clear",
    "ResetSorter",
    "CreateBtree\0r[P2]=root iDb=P1 flags=P3",
    "SqlExec",
    "ParseSchema",
    "LoadAnalysis",
    "DropTable",
    "DropIndex",
    "Real\0r[P2]=PDBL13",
    "DropTrigger",
    "IntegrityCk",
    "RowSetAdd\0rowset(P1)=r[P2]",
    "Param",
    "FkCounter\0fkctr[P1]+=P2",
    "MemMax\0r[P1]=max(r[P1],r[P2])",
    "OffsetLimit\0if r[P1]>0 then r[P2]=r[P1]+max(0,r[P3]) else r[P2]=(-1)",
    "AggInverse\0accum=r[P3] inverse(r[P2@P5])",
    "AggStep\0accum=r[P3] step(r[P2@P5])",
    "AggStep1\0accum=r[P3] step(r[P2@P5])",
    "AggValue\0r[P3]=value N=P2",
    "AggFinal\0accum=r[P1] N=P2",
    "Expire",
    "CursorLock",
    "CursorUnlock",
    "TableLock\0iDb=P1 root=P2 write=P3",
    "VBegin",
    "VCreate",
    "VDestroy",
    "VOpen",
    "VCheck",
    "VInitIn\0r[P2]=ValueList(P1,P3)",
    "VColumn\0r[P3]=vcolumn(P2)",
    "VRename",
    "Pagecount",
    "MaxPgcnt",
    "ClrSubtype\0r[P1].subtype = 0",
    "GetSubtype\0r[P2] = r[P1].subtype",
    "SetSubtype\0r[P2].subtype = r[P1]",
    "FilterAdd\0filter(P1) += key(P3@P4)",
    "Trace",
    "CursorHint",
    "ReleaseReg\0release r[P1@P2] mask P3",
    "Noop",
    "Explain",
    "Abortable",
];

/// sqlite3OpcodeName()
pub fn sqlite3OpcodeName(i: i32) -> &'static str {
    let s = sqlite3OpcodeNames.get(i as usize).copied().unwrap_or("");
    match s.find('\0') {
        Some(p) => &s[..p],
        None => s,
    }
}

/// The synopsis of an opcode (text after the NUL), if any.
pub fn sqlite3OpcodeSynopsis(i: i32) -> Option<&'static str> {
    let s = sqlite3OpcodeNames.get(i as usize).copied()?;
    s.find('\0').map(|p| &s[p + 1..])
}

pub const sqlite3StrBINARY: &[u8] = b"BINARY";
pub const sqlite3StdTypeLen: [u8; 6] = [3, 4, 3, 7, 4, 4];
pub const sqlite3StdTypeAffinity: [u8; 6] = [
    SQLITE_AFF_NUMERIC,
    SQLITE_AFF_BLOB,
    SQLITE_AFF_INTEGER,
    SQLITE_AFF_INTEGER,
    SQLITE_AFF_REAL,
    SQLITE_AFF_TEXT,
];
pub const sqlite3StdType: [&[u8]; 6] = [b"ANY", b"BLOB", b"INT", b"INTEGER", b"REAL", b"TEXT"];

/// The byte offset of the lock-byte page (SQLITE_PENDING_BYTE).
pub const sqlite3PendingByte: i64 = 0x40000000;

/// Global configuration (struct Sqlite3Config); compile-time defaults.
#[derive(Clone, Debug)]
pub struct Sqlite3Config {
    pub bMemstat: i32,
    pub bCoreMutex: u8,
    pub bFullMutex: u8,
    pub bOpenUri: u8,
    pub bUseCis: u8,
    pub bSmallMalloc: u8,
    pub bExtraSchemaChecks: u8,
    pub mxStrlen: i32,
    pub neverCorrupt: i32,
    pub szLookaside: i32,
    pub nLookaside: i32,
    pub nStmtSpill: i32,
    pub szMmap: i64,
    pub mxMmap: i64,
    pub szPage: i32,
    pub nPage: i32,
    pub mxParserStack: i32,
    pub sharedCacheEnabled: i32,
    pub szPma: u32,
    pub mxMemdbSize: i64,
    pub bLocaltimeFault: i32,
    pub iOnceResetThreshold: i32,
    pub szSorterRef: u32,
    pub iPrngSeed: u32,
}

pub static sqlite3Config: Sqlite3Config = Sqlite3Config {
    bMemstat: 0,
    bCoreMutex: 1,
    bFullMutex: 1,
    bOpenUri: 0,
    bUseCis: 1,
    bSmallMalloc: 0,
    bExtraSchemaChecks: 1,
    mxStrlen: 0x7ffffffe,
    neverCorrupt: 0,
    szLookaside: 1200,
    nLookaside: 40,
    nStmtSpill: 64 * 1024,
    szMmap: 0,
    mxMmap: SQLITE_MAX_MMAP_SIZE as i64,
    szPage: 0,
    nPage: 20,
    mxParserStack: 0,
    sharedCacheEnabled: 0,
    szPma: 250,
    mxMemdbSize: 1073741824,
    bLocaltimeFault: 0,
    iOnceResetThreshold: 0x7ffffffe,
    szSorterRef: 0x7fffffff,
    iPrngSeed: 0,
};

/// sqlite3GlobalConfig
#[inline]
pub fn sqlite3GlobalConfig() -> &'static Sqlite3Config {
    &sqlite3Config
}
