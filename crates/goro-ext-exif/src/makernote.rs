//! Maker notes: the vendor IFDs inside the Exif MakerNote tag.
//!
//! The layouts (an optional identifier, where the IFD starts, which base
//! value offsets count from, the byte order) are those the vendors use
//! and are publicly documented by Exif tool authors; which makers PHP
//! reads and the tag names it prints were observed on PHP 8.5 with the
//! ext/exif test images.

/// A maker whose notes are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maker {
    Canon,
    Fujifilm,
    Nikon,
    Olympus,
    Samsung,
    Panasonic,
    Sony,
    Minolta,
}

/// Byte order of the maker note IFD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Same,
    Intel,
    Motorola,
}

/// How to read one maker's note.
#[derive(Debug, Clone, Copy)]
pub struct Desc {
    pub maker: Maker,
    /// Prefix of the Make tag.
    pub make: &'static [u8],
    /// Identifier at the start of the note.
    pub id: &'static [u8],
    /// Where the IFD starts in the note.
    pub ifd_offset: usize,
    /// Value offsets count from the note's start (else from the TIFF
    /// header).
    pub relative: bool,
    pub order: Order,
}

/// Where the maker note being read sits.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub start: usize,
    pub offsets_relative: bool,
}

const DESCS: &[Desc] = &[
    Desc {
        maker: Maker::Canon,
        make: b"Canon",
        id: b"",
        ifd_offset: 0,
        relative: false,
        order: Order::Same,
    },
    Desc {
        maker: Maker::Fujifilm,
        make: b"FUJIFILM",
        id: b"FUJIFILM\x0C\x00\x00\x00",
        ifd_offset: 12,
        relative: true,
        order: Order::Intel,
    },
    Desc {
        maker: Maker::Nikon,
        make: b"NIKON",
        id: b"Nikon\x00\x01\x00",
        ifd_offset: 8,
        relative: false,
        order: Order::Same,
    },
    Desc {
        maker: Maker::Olympus,
        make: b"OLYMPUS OPTICAL CO.,LTD",
        id: b"OLYMP\x00\x01\x00",
        ifd_offset: 8,
        relative: false,
        order: Order::Same,
    },
    Desc {
        maker: Maker::Samsung,
        make: b"SAMSUNG",
        id: b"",
        ifd_offset: 0,
        relative: false,
        order: Order::Same,
    },
    Desc {
        maker: Maker::Panasonic,
        make: b"Panasonic",
        id: b"Panasonic\x00\x00\x00",
        ifd_offset: 12,
        relative: false,
        order: Order::Same,
    },
    Desc {
        maker: Maker::Sony,
        make: b"SONY",
        id: b"SONY DSC \x00\x00\x00",
        ifd_offset: 12,
        relative: false,
        order: Order::Same,
    },
    Desc {
        maker: Maker::Minolta,
        make: b"Minolta, KONICA MINOLTA",
        id: b"",
        ifd_offset: 0,
        relative: false,
        order: Order::Same,
    },
];

/// The layout for a note with this Make and data.
pub fn find(make: &[u8], data: &[u8]) -> Option<&'static Desc> {
    DESCS.iter().find(|d| {
        make.starts_with(d.make) && (d.id.is_empty() || data.starts_with(d.id))
    })
}

/// Tag names (observed on PHP 8.5).
pub fn table(m: Maker) -> &'static [(u16, &'static str)] {
    match m {
        Maker::Canon => CANON,
        Maker::Fujifilm => FUJIFILM,
        Maker::Nikon => NIKON,
        Maker::Olympus => OLYMPUS,
        Maker::Samsung => SAMSUNG,
        Maker::Panasonic => PANASONIC,
        Maker::Sony => SONY,
        Maker::Minolta => MINOLTA,
    }
}

/// Canon (test bug62523_1.phpt images).
static CANON: &[(u16, &str)] = &[
    (0x0001, "ModeArray"),
    (0x0004, "ImageInfo"),
    (0x0006, "ImageType"),
    (0x0007, "FirmwareVersion"),
    (0x0008, "ImageNumber"),
    (0x0009, "OwnerName"),
    (0x000C, "Camera"),
    (0x000F, "CustomFunctions"),
];
static FUJIFILM: &[(u16, &str)] = &[];
static NIKON: &[(u16, &str)] = &[];
/// Olympus (test bug54002.phpt images).
static OLYMPUS: &[(u16, &str)] = &[
    (0x0201, "JPEGQuality"),
    (0x0202, "Macro"),
    (0x0204, "DigitalZoom"),
];
/// Samsung (test bug72735.phpt images).
static SAMSUNG: &[(u16, &str)] = &[
    (0x0001, "Version"),
];
/// Panasonic (tests bug72735.phpt, bug75785.phpt). Two ids share some names: the later value takes the earlier key's place.
static PANASONIC: &[(u16, &str)] = &[
    (0x0001, "Quality"),
    (0x0002, "FirmwareVersion"),
    (0x0003, "WhiteBalance"),
    (0x0007, "FocusMode"),
    (0x000F, "AFMode"),
    (0x001A, "ImageStabilization"),
    (0x001C, "Macro"),
    (0x001F, "ShootingMode"),
    (0x0020, "Audio"),
    (0x0021, "DataDump"),
    (0x0023, "WhiteBalanceBias"),
    (0x0024, "FlashBias"),
    (0x0025, "InternalSerialNumber"),
    (0x0026, "ExifVersion"),
    (0x0028, "ColorEffect"),
    (0x0029, "TimeSincePowerOn"),
    (0x002A, "BurstMode"),
    (0x002B, "SequenceNumber"),
    (0x002C, "Contrast"),
    (0x002D, "NoiseReduction"),
    (0x002E, "SelfTimer"),
    (0x0030, "Rotation"),
    (0x0031, "AFAssistLamp"),
    (0x0032, "ColorMode"),
    (0x0033, "BabyAge1"),
    (0x0034, "OpticalZoomMode"),
    (0x0035, "ConversionLens"),
    (0x0036, "TravelDay"),
    (0x0039, "Contrast"),
    (0x003A, "WorldTimeLocation"),
    (0x003B, "TextStamp1"),
    (0x003C, "ProgramISO"),
    (0x003D, "AdvancedSceneType"),
    (0x003E, "TextStamp2"),
    (0x003F, "FacesDetected"),
    (0x0040, "Saturation"),
    (0x0041, "Sharpness"),
    (0x0049, "LongShutterNoiseReduction"),
    (0x004D, "AFPointPosition"),
    (0x004E, "FaceDetInfo"),
    (0x0059, "Transform1"),
    (0x005D, "IntelligentExposure"),
    (0x0061, "FaceRecInfo"),
    (0x0062, "FlashWarning"),
    (0x0065, "Title"),
    (0x0066, "BabyName"),
    (0x0067, "Location"),
    (0x0069, "Country"),
    (0x006B, "State"),
    (0x006D, "City"),
    (0x006F, "Landmark"),
    (0x0070, "IntelligentResolution"),
    (0x8000, "MakerNoteVersion"),
    (0x8001, "SceneMode"),
    (0x8004, "WBRedLevel"),
    (0x8005, "WBGreenLevel"),
    (0x8006, "WBBlueLevel"),
    (0x8007, "FlashFired"),
    (0x8008, "TextStamp3"),
    (0x8009, "TextStamp4"),
    (0x8010, "BabyAge2"),
    (0x8012, "Transform2"),
];
/// Sony (test sony.phpt). Two ids share some names.
static SONY: &[(u16, &str)] = &[
    (0x0102, "Quality"),
    (0x0104, "FlashExposureComp"),
    (0x0112, "WhiteBalanceFineTune"),
    (0x0115, "WhiteBalance"),
    (0x1003, "Panorama"),
    (0x2002, "Rating"),
    (0x2004, "Contrast"),
    (0x2005, "Saturation"),
    (0x2006, "Sharpness"),
    (0x2007, "Brightness"),
    (0x2008, "LongExposureNoiseReduction"),
    (0x2009, "HighISONoiseReduction"),
    (0x200A, "AutoHDR"),
    (0x3000, "ShotInfo"),
    (0xB000, "FileFormat"),
    (0xB001, "SonyModelID"),
    (0xB020, "ColorReproduction"),
    (0xB021, "ColorTemperature"),
    (0xB022, "ColorCompensationFilter"),
    (0xB023, "SceneMode"),
    (0xB024, "ZoneMatching"),
    (0xB025, "DynamicRangeOptimizer"),
    (0xB026, "ImageStabilization"),
    (0xB029, "ColorMode"),
    (0xB02B, "FullImageSize"),
    (0xB02C, "PreviewImageSize"),
    (0xB040, "Macro"),
    (0xB041, "ExposureMode"),
    (0xB042, "FocusMode"),
    (0xB043, "AFMode"),
    (0xB044, "AFIlluminator"),
    (0xB047, "JPEGQuality"),
    (0xB048, "FlashLevel"),
    (0xB049, "ReleaseMode"),
    (0xB04A, "SequenceNumber"),
    (0xB04B, "AntiBlur"),
    (0xB04E, "FocusMode"),
    (0xB04F, "DynamicRangeOptimizer"),
    (0xB050, "HighISONoiseReduction2"),
    (0xB052, "IntelligentAuto"),
    (0xB054, "WhiteBalance2"),
];
static MINOLTA: &[(u16, &str)] = &[];
