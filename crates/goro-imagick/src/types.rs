//! The enumerations of ImageMagick 7's public API (MagickCore headers), as
//! integer newtypes so that the PHP binding can pass its constants through.

macro_rules! int_enum {
    ($name:ident { $($c:ident = $v:expr),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
        pub struct $name(pub i32);
        #[allow(non_upper_case_globals)]
        impl $name {
            $(pub const $c: $name = $name($v);)*
        }
    };
}

int_enum!(Colorspace {
    Undefined = 0, CMY = 1, CMYK = 2, Gray = 3, HCL = 4, HCLp = 5, HSB = 6, HSI = 7,
    HSL = 8, HSV = 9, HWB = 10, Lab = 11, LCH = 12, LCHab = 13, LCHuv = 14, Log = 15,
    LMS = 16, Luv = 17, OHTA = 18, Rec601YCbCr = 19, Rec709YCbCr = 20, RGB = 21,
    scRGB = 22, SRGB = 23, Transparent = 24, XyY = 25, XYZ = 26, YCbCr = 27, YCC = 28,
    YDbDr = 29, YIQ = 30, YPbPr = 31, YUV = 32, LinearGray = 33, Jzazbz = 34,
    DisplayP3 = 35, Adobe98 = 36, ProPhoto = 37, Oklab = 38, Oklch = 39,
});

impl Colorspace {
    pub fn name(self) -> &'static str {
        const N: &[&str] = &[
            "Undefined",
            "CMY",
            "CMYK",
            "Gray",
            "HCL",
            "HCLp",
            "HSB",
            "HSI",
            "HSL",
            "HSV",
            "HWB",
            "Lab",
            "LCH",
            "LCHab",
            "LCHuv",
            "Log",
            "LMS",
            "Luv",
            "OHTA",
            "Rec601YCbCr",
            "Rec709YCbCr",
            "RGB",
            "scRGB",
            "sRGB",
            "Transparent",
            "xyY",
            "XYZ",
            "YCbCr",
            "YCC",
            "YDbDr",
            "YIQ",
            "YPbPr",
            "YUV",
            "LinearGray",
            "Jzazbz",
            "DisplayP3",
            "Adobe98",
            "ProPhoto",
            "Oklab",
            "Oklch",
        ];
        N.get(self.0 as usize).copied().unwrap_or("Undefined")
    }
}

int_enum!(ImageType {
    Undefined = 0, Bilevel = 1, Grayscale = 2, GrayscaleAlpha = 3, Palette = 4,
    PaletteAlpha = 5, TrueColor = 6, TrueColorAlpha = 7, ColorSeparation = 8,
    ColorSeparationAlpha = 9, Optimize = 10, PaletteBilevelAlpha = 11,
});

impl ImageType {
    pub fn name(self) -> &'static str {
        const N: &[&str] = &[
            "Undefined",
            "Bilevel",
            "Grayscale",
            "GrayscaleAlpha",
            "Palette",
            "PaletteAlpha",
            "TrueColor",
            "TrueColorAlpha",
            "ColorSeparation",
            "ColorSeparationAlpha",
            "Optimize",
            "PaletteBilevelAlpha",
        ];
        N.get(self.0 as usize).copied().unwrap_or("Undefined")
    }
}

int_enum!(CompositeOp {
    Undefined = 0, Alpha = 1, Atop = 2, Blend = 3, Blur = 4, Bumpmap = 5, ChangeMask = 6,
    Clear = 7, ColorBurn = 8, ColorDodge = 9, Colorize = 10, CopyBlack = 11, CopyBlue = 12,
    Copy = 13, CopyCyan = 14, CopyGreen = 15, CopyMagenta = 16, CopyAlpha = 17, CopyRed = 18,
    CopyYellow = 19, Darken = 20, DarkenIntensity = 21, Difference = 22, Displace = 23,
    Dissolve = 24, Distort = 25, DivideDst = 26, DivideSrc = 27, DstAtop = 28, Dst = 29,
    DstIn = 30, DstOut = 31, DstOver = 32, Exclusion = 33, HardLight = 34, HardMix = 35,
    Hue = 36, In = 37, Intensity = 38, Lighten = 39, LightenIntensity = 40, LinearBurn = 41,
    LinearDodge = 42, LinearLight = 43, Luminize = 44, Mathematics = 45, MinusDst = 46,
    MinusSrc = 47, Modulate = 48, ModulusAdd = 49, ModulusSubtract = 50, Multiply = 51,
    No = 52, Out = 53, Over = 54, Overlay = 55, PegtopLight = 56, PinLight = 57, Plus = 58,
    Replace = 59, Saturate = 60, Screen = 61, SoftLight = 62, SrcAtop = 63, Src = 64,
    SrcIn = 65, SrcOut = 66, SrcOver = 67, Threshold = 68, VividLight = 69, Xor = 70,
    Stereo = 71, Freeze = 72, Interpolate = 73, Negate = 74, Reflect = 75, SoftBurn = 76,
    SoftDodge = 77, Stamp = 78, RMSE = 79, SaliencyBlend = 80, SeamlessBlend = 81,
});

int_enum!(FilterType {
    Undefined = 0, Point = 1, Box = 2, Triangle = 3, Hermite = 4, Hann = 5, Hamming = 6,
    Blackman = 7, Gaussian = 8, Quadratic = 9, Cubic = 10, Catrom = 11, Mitchell = 12,
    Jinc = 13, Sinc = 14, SincFast = 15, Kaiser = 16, Welch = 17, Parzen = 18, Bohman = 19,
    Bartlett = 20, Lagrange = 21, Lanczos = 22, LanczosSharp = 23, Lanczos2 = 24,
    Lanczos2Sharp = 25, Robidoux = 26, RobidouxSharp = 27, Cosine = 28, Spline = 29,
    LanczosRadius = 30, CubicSpline = 31, MagicKernelSharp2013 = 32, MagicKernelSharp2021 = 33,
    Sentinel = 34,
});

int_enum!(Gravity {
    Undefined = 0, NorthWest = 1, North = 2, NorthEast = 3, West = 4, Center = 5, East = 6,
    SouthWest = 7, South = 8, SouthEast = 9,
});

impl Gravity {
    pub fn name(self) -> &'static str {
        const N: &[&str] = &[
            "Undefined",
            "NorthWest",
            "North",
            "NorthEast",
            "West",
            "Center",
            "East",
            "SouthWest",
            "South",
            "SouthEast",
        ];
        N.get(self.0 as usize).copied().unwrap_or("Undefined")
    }
}

int_enum!(VirtualPixelMethod {
    Undefined = 0, Background = 1, Dither = 2, Edge = 3, Mirror = 4, Random = 5, Tile = 6,
    Transparent = 7, Mask = 8, Black = 9, Gray = 10, White = 11, HorizontalTile = 12,
    VerticalTile = 13, HorizontalTileEdge = 14, VerticalTileEdge = 15, CheckerTile = 16,
});

int_enum!(AlphaChannelOption {
    Undefined = 0, Activate = 1, Associate = 2, Background = 3, Copy = 4, Deactivate = 5,
    Discrete = 6, Disassociate = 7, Extract = 8, Off = 9, On = 10, Opaque = 11, Remove = 12,
    Set = 13, Shape = 14, Transparent = 15, OffIfOpaque = 16,
});

int_enum!(CompressionType {
    Undefined = 0, B44A = 1, B44 = 2, BZip = 3, DXT1 = 4, DXT3 = 5, DXT5 = 6, Fax = 7,
    Group4 = 8, JBIG1 = 9, JBIG2 = 10, JPEG2000 = 11, JPEG = 12, LosslessJPEG = 13,
    LZMA = 14, LZW = 15, No = 16, Piz = 17, Pxr24 = 18, RLE = 19, Zip = 20, ZipS = 21,
    Zstd = 22, WebP = 23, DWAA = 24, DWAB = 25, BC7 = 26, BC5 = 27, LERC = 28,
});

impl CompressionType {
    pub fn name(self) -> &'static str {
        const N: &[&str] = &[
            "Undefined",
            "B44A",
            "B44",
            "BZip",
            "DXT1",
            "DXT3",
            "DXT5",
            "Fax",
            "Group4",
            "JBIG1",
            "JBIG2",
            "JPEG2000",
            "JPEG",
            "LosslessJPEG",
            "LZMA",
            "LZW",
            "None",
            "Piz",
            "Pxr24",
            "RLE",
            "Zip",
            "ZipS",
            "Zstd",
            "WebP",
            "DWAA",
            "DWAB",
            "BC7",
            "BC5",
            "LERC",
        ];
        N.get(self.0 as usize).copied().unwrap_or("Undefined")
    }
}

int_enum!(InterlaceType {
    Undefined = 0, No = 1, Line = 2, Plane = 3, Partition = 4, GIF = 5, JPEG = 6, PNG = 7,
});

int_enum!(OrientationType {
    Undefined = 0, TopLeft = 1, TopRight = 2, BottomRight = 3, BottomLeft = 4, LeftTop = 5,
    RightTop = 6, RightBottom = 7, LeftBottom = 8,
});

impl OrientationType {
    pub fn name(self) -> &'static str {
        const N: &[&str] = &[
            "Undefined",
            "TopLeft",
            "TopRight",
            "BottomRight",
            "BottomLeft",
            "LeftTop",
            "RightTop",
            "RightBottom",
            "LeftBottom",
        ];
        N.get(self.0 as usize).copied().unwrap_or("Undefined")
    }
}

int_enum!(DisposeType {
    Undefined = 0, No = 1, Background = 2, Previous = 3,
});

int_enum!(ResolutionType {
    Undefined = 0, PixelsPerInch = 1, PixelsPerCentimeter = 2,
});

impl ResolutionType {
    pub fn name(self) -> &'static str {
        match self.0 {
            1 => "PixelsPerInch",
            2 => "PixelsPerCentimeter",
            _ => "Undefined",
        }
    }
}

int_enum!(PixelInterpolateMethod {
    Undefined = 0, Average = 1, Average9 = 2, Average16 = 3, Background = 4, Bilinear = 5,
    Blend = 6, Catrom = 7, Integer = 8, Mesh = 9, Nearest = 10, Spline = 11,
});

int_enum!(StorageType {
    Undefined = 0, Char = 1, Double = 2, Float = 3, Long = 4, LongLong = 5, Quantum = 6,
    Short = 7,
});

int_enum!(MetricType {
    Undefined = 0, AbsoluteError = 1, Fuzz = 2, MeanAbsoluteError = 3,
    MeanErrorPerPixel = 4, MeanSquaredError = 5, NormalizedCrossCorrelationError = 6,
    PeakAbsoluteError = 7, PeakSignalToNoiseRatio = 8, PerceptualHash = 9,
    RootMeanSquaredError = 10, StructuralSimilarity = 11, StructuralDissimilarity = 12,
    PhaseCorrelation = 13, DotProductCorrelation = 14,
});

int_enum!(NoiseType {
    Undefined = 0, Uniform = 1, Gaussian = 2, MultiplicativeGaussian = 3, Impulse = 4,
    Laplacian = 5, Poisson = 6, Random = 7,
});

/// ChannelType flags.
pub mod channel {
    pub const UNDEFINED: i64 = 0;
    pub const RED: i64 = 0x0001;
    pub const GREEN: i64 = 0x0002;
    pub const BLUE: i64 = 0x0004;
    pub const BLACK: i64 = 0x0008;
    pub const ALPHA: i64 = 0x0010;
    pub const INDEX: i64 = 0x0020;
    pub const ALL: i64 = 0x7ffffff;
    pub const DEFAULT: i64 = ALL;
}
