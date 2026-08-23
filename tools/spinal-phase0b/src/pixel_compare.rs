//! Deterministic comparison of the frozen Phase 0B v1 browser PNG references.
//!
//! Inputs are complete, bounded, static 640-by-480 RGB8 or RGBA8 PNG byte
//! strings. RGB inputs are expanded to opaque RGBA in memory; encoded inputs
//! are never rewritten. The policy is deliberately fixed rather than
//! caller-configurable, and the resulting comparison is explicitly ineligible
//! to decide the Phase 0B gate.

use std::fmt;
use std::io::Cursor;

use flate2::{Decompress, FlushDecompress, Status};
use png::{BitDepth, ColorType, DecodeOptions, Decoder, Limits};
use serde::Serialize;
use thiserror::Error;

/// The structured pixel-comparison report schema emitted by this module.
pub const PIXEL_COMPARISON_FORMAT_VERSION: u16 = 1;

/// The maximum encoded byte length accepted for either PNG input.
pub const MAX_ENCODED_PNG_BYTES: usize = 4 * 1024 * 1024;

/// The required browser-reference width in pixels.
pub const REQUIRED_WIDTH: u32 = 640;

/// The required browser-reference height in pixels.
pub const REQUIRED_HEIGHT: u32 = 480;

/// A channel delta at or below this value does not mark a pixel as changed.
pub const MAX_UNCHANGED_CHANNEL_DELTA: u8 = 8;

const CHANNELS_PER_PIXEL: usize = 4;
const RGB_CHANNELS_PER_PIXEL: usize = 3;
const DECODED_RGB_BYTES: usize =
    REQUIRED_WIDTH as usize * REQUIRED_HEIGHT as usize * RGB_CHANNELS_PER_PIXEL;
const DECODED_RGBA_BYTES: usize =
    REQUIRED_WIDTH as usize * REQUIRED_HEIGHT as usize * CHANNELS_PER_PIXEL;
const CHANGED_FRACTION_NUMERATOR: u64 = 2;
const CHANGED_FRACTION_DENOMINATOR: u64 = 100;
const MEAN_DELTA_NUMERATOR: u64 = 1;
const MEAN_DELTA_DENOMINATOR: u64 = 2;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// The fixed maximum changed-pixel fraction.
pub const MAX_CHANGED_PIXEL_FRACTION: f64 =
    CHANGED_FRACTION_NUMERATOR as f64 / CHANGED_FRACTION_DENOMINATOR as f64;

/// The fixed maximum mean absolute RGBA-channel delta.
pub const MAX_MEAN_ABSOLUTE_CHANNEL_DELTA: f64 =
    MEAN_DELTA_NUMERATOR as f64 / MEAN_DELTA_DENOMINATOR as f64;

/// Identifies which comparison input was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelComparisonInput {
    /// The checked reference PNG.
    Expected,
    /// The observed PNG being compared with the reference.
    Actual,
}

impl fmt::Display for PixelComparisonInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expected => formatter.write_str("expected"),
            Self::Actual => formatter.write_str("actual"),
        }
    }
}

/// The decode phase in which a structurally invalid PNG was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PngDecodeStage {
    /// PNG signature or chunk framing.
    Structure,
    /// PNG metadata and decoder initialization.
    Metadata,
    /// Compressed image data or decoded pixels.
    Pixels,
    /// The final PNG chunks and stream termination.
    Ending,
}

impl fmt::Display for PngDecodeStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Structure => formatter.write_str("structure"),
            Self::Metadata => formatter.write_str("metadata"),
            Self::Pixels => formatter.write_str("pixels"),
            Self::Ending => formatter.write_str("ending"),
        }
    }
}

/// A PNG input was rejected before a comparison could be produced.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum PixelCompareError {
    /// An encoded input exceeds the fixed four-mebibyte limit.
    #[error("{input} PNG has {encoded_bytes} encoded bytes; limit is {max_encoded_bytes}")]
    EncodedByteLimit {
        /// The rejected comparison side.
        input: PixelComparisonInput,
        /// The supplied encoded byte length.
        encoded_bytes: usize,
        /// The fixed maximum encoded byte length.
        max_encoded_bytes: usize,
    },
    /// The PNG signature, chunks, compressed stream, or ending is malformed.
    #[error("{input} PNG has invalid {stage}")]
    MalformedPng {
        /// The rejected comparison side.
        input: PixelComparisonInput,
        /// The phase in which validation failed.
        stage: PngDecodeStage,
    },
    /// A PNG chunk checksum is invalid.
    #[error("{input} PNG chunk {chunk_index} has an invalid CRC")]
    ChecksumMismatch {
        /// The rejected comparison side.
        input: PixelComparisonInput,
        /// Zero-based index of the corrupt chunk.
        chunk_index: usize,
    },
    /// The PNG is not in a fixed static, non-interlaced RGB8 or RGBA8 profile.
    #[error("{input} PNG does not use a required static non-interlaced RGB8 or RGBA8 profile")]
    UnsupportedProfile {
        /// The rejected comparison side.
        input: PixelComparisonInput,
    },
    /// The PNG dimensions are not exactly 640 by 480.
    #[error(
        "{input} PNG dimensions are {width}x{height}; required dimensions are {required_width}x{required_height}"
    )]
    Dimensions {
        /// The rejected comparison side.
        input: PixelComparisonInput,
        /// Width declared by the PNG.
        width: u32,
        /// Height declared by the PNG.
        height: u32,
        /// The fixed required width.
        required_width: u32,
        /// The fixed required height.
        required_height: u32,
    },
    /// Bytes occur after the final IEND chunk.
    #[error("{input} PNG has {trailing_bytes} trailing bytes after IEND")]
    TrailingData {
        /// The rejected comparison side.
        input: PixelComparisonInput,
        /// Number of bytes after the complete IEND chunk.
        trailing_bytes: usize,
    },
}

/// Deterministic metrics for two complete decoded browser images.
///
/// Agreement means only that the frozen generic v1 pixel policy was met. It is
/// never representative evidence and cannot make the authoritative gate
/// eligible.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PixelComparison {
    format_version: u16,
    width: u32,
    height: u32,
    pixel_count: u64,
    channel_count: u64,
    changed_pixel_count: u64,
    changed_pixel_fraction: f64,
    mean_absolute_channel_delta: f64,
    max_observed_channel_delta: u8,
    agrees: bool,
}

impl PixelComparison {
    /// Returns the comparison-report schema version.
    #[must_use]
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }

    /// Returns the common decoded image width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the common decoded image height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the number of compared pixels.
    #[must_use]
    pub const fn pixel_count(&self) -> u64 {
        self.pixel_count
    }

    /// Returns the number of compared RGBA channels.
    #[must_use]
    pub const fn channel_count(&self) -> u64 {
        self.channel_count
    }

    /// Returns pixels for which at least one channel delta is greater than 8.
    #[must_use]
    pub const fn changed_pixel_count(&self) -> u64 {
        self.changed_pixel_count
    }

    /// Returns changed pixels divided by all compared pixels.
    #[must_use]
    pub const fn changed_pixel_fraction(&self) -> f64 {
        self.changed_pixel_fraction
    }

    /// Returns the mean absolute delta across all compared RGBA channels.
    #[must_use]
    pub const fn mean_absolute_channel_delta(&self) -> f64 {
        self.mean_absolute_channel_delta
    }

    /// Returns the largest absolute delta observed in any RGBA channel.
    #[must_use]
    pub const fn max_observed_channel_delta(&self) -> u8 {
        self.max_observed_channel_delta
    }

    /// Returns whether both fixed generic v1 pixel tolerances were met.
    #[must_use]
    pub const fn agrees(&self) -> bool {
        self.agrees
    }

    /// Returns whether this result is eligible to decide the authoritative gate.
    ///
    /// Generic v1 pixel comparisons are categorically gate-ineligible.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        false
    }
}

/// Strictly decodes and compares two complete frozen-v1 browser PNGs.
///
/// The encoded byte limit, dimensions, PNG profile, and comparison thresholds
/// are fixed. No files are read and no diff image is generated.
pub fn compare_browser_pngs(
    expected_png: &[u8],
    actual_png: &[u8],
) -> Result<PixelComparison, PixelCompareError> {
    let expected = decode_png(PixelComparisonInput::Expected, expected_png)?;
    let actual = decode_png(PixelComparisonInput::Actual, actual_png)?;

    let pixel_count = u64::from(expected.width) * u64::from(expected.height);
    let channel_count = pixel_count * CHANNELS_PER_PIXEL as u64;
    let mut changed_pixel_count = 0_u64;
    let mut absolute_channel_delta_sum = 0_u64;
    let mut max_observed_channel_delta = 0_u8;

    for (expected_pixel, actual_pixel) in expected
        .pixels
        .as_chunks::<CHANNELS_PER_PIXEL>()
        .0
        .iter()
        .zip(actual.pixels.as_chunks::<CHANNELS_PER_PIXEL>().0.iter())
    {
        let mut changed = false;
        for (&expected_channel, &actual_channel) in expected_pixel.iter().zip(actual_pixel) {
            let delta = expected_channel.abs_diff(actual_channel);
            absolute_channel_delta_sum += u64::from(delta);
            max_observed_channel_delta = max_observed_channel_delta.max(delta);
            changed |= delta > MAX_UNCHANGED_CHANNEL_DELTA;
        }
        changed_pixel_count += u64::from(changed);
    }

    // Integer cross-products make the inclusive boundary decisions exact;
    // floating-point values below are report metrics, not decision inputs.
    let changed_fraction_within_policy = changed_pixel_count
        .checked_mul(CHANGED_FRACTION_DENOMINATOR)
        .is_some_and(|scaled| scaled <= pixel_count * CHANGED_FRACTION_NUMERATOR);
    let mean_delta_within_policy = absolute_channel_delta_sum
        .checked_mul(MEAN_DELTA_DENOMINATOR)
        .is_some_and(|scaled| scaled <= channel_count * MEAN_DELTA_NUMERATOR);

    Ok(PixelComparison {
        format_version: PIXEL_COMPARISON_FORMAT_VERSION,
        width: expected.width,
        height: expected.height,
        pixel_count,
        channel_count,
        changed_pixel_count,
        changed_pixel_fraction: changed_pixel_count as f64 / pixel_count as f64,
        mean_absolute_channel_delta: absolute_channel_delta_sum as f64 / channel_count as f64,
        max_observed_channel_delta,
        agrees: changed_fraction_within_policy && mean_delta_within_policy,
    })
}

struct DecodedPng {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcceptedPngProfile {
    Rgb,
    Rgba,
}

impl AcceptedPngProfile {
    const fn color_type(self) -> ColorType {
        match self {
            Self::Rgb => ColorType::Rgb,
            Self::Rgba => ColorType::Rgba,
        }
    }

    const fn decoded_bytes(self) -> usize {
        match self {
            Self::Rgb => DECODED_RGB_BYTES,
            Self::Rgba => DECODED_RGBA_BYTES,
        }
    }

    const fn significant_bits_bytes(self) -> usize {
        match self {
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    const fn inflated_scanline_bytes(self) -> usize {
        REQUIRED_HEIGHT as usize
            * (1 + REQUIRED_WIDTH as usize
                * match self {
                    Self::Rgb => RGB_CHANNELS_PER_PIXEL,
                    Self::Rgba => CHANNELS_PER_PIXEL,
                })
    }
}

fn decode_png(input: PixelComparisonInput, bytes: &[u8]) -> Result<DecodedPng, PixelCompareError> {
    if bytes.len() > MAX_ENCODED_PNG_BYTES {
        return Err(PixelCompareError::EncodedByteLimit {
            input,
            encoded_bytes: bytes.len(),
            max_encoded_bytes: MAX_ENCODED_PNG_BYTES,
        });
    }
    if bytes.len() < 33 || &bytes[..8] != PNG_SIGNATURE {
        return Err(malformed(input, PngDecodeStage::Structure));
    }

    let width = u32::from_be_bytes(
        bytes[16..20]
            .try_into()
            .expect("four-byte PNG width after minimum-length check"),
    );
    let height = u32::from_be_bytes(
        bytes[20..24]
            .try_into()
            .expect("four-byte PNG height after minimum-length check"),
    );
    let (profile, compressed_image_data) = validate_chunk_profile_and_checksums(input, bytes)?;

    if width != REQUIRED_WIDTH || height != REQUIRED_HEIGHT {
        return Err(PixelCompareError::Dimensions {
            input,
            width,
            height,
            required_width: REQUIRED_WIDTH,
            required_height: REQUIRED_HEIGHT,
        });
    }
    validate_exact_zlib_stream(input, profile, &compressed_image_data)?;

    let mut options = DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_skip_ancillary_crc_failures(false);
    let mut decoder = Decoder::new_with_options(Cursor::new(bytes), options);
    decoder.set_limits(Limits {
        bytes: DECODED_RGBA_BYTES,
    });
    let mut reader = decoder
        .read_info()
        .map_err(|_error| malformed(input, PngDecodeStage::Metadata))?;
    let info = reader.info();
    if info.width != width
        || info.height != height
        || info.bit_depth != BitDepth::Eight
        || info.color_type != profile.color_type()
        || info.interlaced
        || info.animation_control.is_some()
    {
        return Err(PixelCompareError::UnsupportedProfile { input });
    }
    let output_bytes = reader
        .output_buffer_size()
        .ok_or_else(|| malformed(input, PngDecodeStage::Pixels))?;
    if output_bytes != profile.decoded_bytes() {
        return Err(PixelCompareError::UnsupportedProfile { input });
    }

    let mut decoded = vec![0_u8; output_bytes];
    let output = reader
        .next_frame(&mut decoded)
        .map_err(|_error| malformed(input, PngDecodeStage::Pixels))?;
    if output.buffer_size() != output_bytes
        || output.color_type != profile.color_type()
        || output.bit_depth != BitDepth::Eight
    {
        return Err(PixelCompareError::UnsupportedProfile { input });
    }
    reader
        .finish()
        .map_err(|_error| malformed(input, PngDecodeStage::Ending))?;

    let pixels = match profile {
        AcceptedPngProfile::Rgba => decoded,
        AcceptedPngProfile::Rgb => {
            let mut rgba = Vec::with_capacity(DECODED_RGBA_BYTES);
            for pixel in decoded.as_chunks::<RGB_CHANNELS_PER_PIXEL>().0 {
                rgba.extend_from_slice(pixel);
                rgba.push(u8::MAX);
            }
            if rgba.len() != DECODED_RGBA_BYTES {
                return Err(PixelCompareError::UnsupportedProfile { input });
            }
            rgba
        }
    };

    Ok(DecodedPng {
        width,
        height,
        pixels,
    })
}

fn malformed(input: PixelComparisonInput, stage: PngDecodeStage) -> PixelCompareError {
    PixelCompareError::MalformedPng { input, stage }
}

fn validate_chunk_profile_and_checksums(
    input: PixelComparisonInput,
    bytes: &[u8],
) -> Result<(AcceptedPngProfile, Vec<u8>), PixelCompareError> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Stage {
        Header,
        Metadata,
        ImageData,
    }

    #[derive(Default)]
    struct MetadataSeen {
        chromaticities: bool,
        gamma: bool,
        significant_bits: bool,
        srgb: bool,
        background: bool,
        pixel_dimensions: bool,
    }

    let mut cursor = PNG_SIGNATURE.len();
    let mut stage = Stage::Header;
    let mut saw_image_data = false;
    let mut chunk_index = 0_usize;
    let mut profile = None;
    let mut compressed_image_data = Vec::new();
    let mut metadata_seen = MetadataSeen::default();

    loop {
        let header_end = cursor
            .checked_add(8)
            .ok_or_else(|| malformed(input, PngDecodeStage::Structure))?;
        if header_end > bytes.len() {
            return Err(malformed(input, PngDecodeStage::Structure));
        }
        let length = usize::try_from(u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .expect("four-byte PNG chunk length"),
        ))
        .map_err(|_error| malformed(input, PngDecodeStage::Structure))?;
        let kind = &bytes[cursor + 4..header_end];
        let data_end = header_end
            .checked_add(length)
            .ok_or_else(|| malformed(input, PngDecodeStage::Structure))?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or_else(|| malformed(input, PngDecodeStage::Structure))?;
        if chunk_end > bytes.len() {
            return Err(malformed(input, PngDecodeStage::Structure));
        }

        let stored_crc = u32::from_be_bytes(
            bytes[data_end..chunk_end]
                .try_into()
                .expect("four-byte PNG chunk CRC"),
        );
        if png_crc32(&bytes[cursor + 4..data_end]) != stored_crc {
            return Err(PixelCompareError::ChecksumMismatch { input, chunk_index });
        }

        match kind {
            b"IHDR" if stage == Stage::Header && cursor == 8 && length == 13 => {
                profile = Some(
                    match (bytes[24], bytes[25], bytes[26], bytes[27], bytes[28]) {
                        (8, 2, 0, 0, 0) => AcceptedPngProfile::Rgb,
                        (8, 6, 0, 0, 0) => AcceptedPngProfile::Rgba,
                        _other => return Err(PixelCompareError::UnsupportedProfile { input }),
                    },
                );
                stage = Stage::Metadata;
            }
            b"cHRM"
                if stage == Stage::Metadata && length == 32 && !metadata_seen.chromaticities =>
            {
                metadata_seen.chromaticities = true;
            }
            b"gAMA"
                if stage == Stage::Metadata
                    && length == 4
                    && !metadata_seen.gamma
                    && bytes[header_end..data_end] != [0, 0, 0, 0] =>
            {
                metadata_seen.gamma = true;
            }
            b"sBIT"
                if stage == Stage::Metadata
                    && !metadata_seen.significant_bits
                    && profile
                        .is_some_and(|profile| length == profile.significant_bits_bytes())
                    && bytes[header_end..data_end]
                        .iter()
                        .all(|value| (1..=8).contains(value)) =>
            {
                metadata_seen.significant_bits = true;
            }
            b"sRGB"
                if stage == Stage::Metadata
                    && length == 1
                    && !metadata_seen.srgb
                    && bytes[header_end] <= 3 =>
            {
                metadata_seen.srgb = true;
            }
            b"bKGD" if stage == Stage::Metadata && length == 6 && !metadata_seen.background => {
                metadata_seen.background = true;
            }
            b"pHYs"
                if stage == Stage::Metadata
                    && length == 9
                    && !metadata_seen.pixel_dimensions
                    && bytes[data_end - 1] <= 1 =>
            {
                metadata_seen.pixel_dimensions = true;
            }
            b"IDAT" if matches!(stage, Stage::Metadata | Stage::ImageData) && length > 0 => {
                stage = Stage::ImageData;
                saw_image_data = true;
                compressed_image_data.extend_from_slice(&bytes[header_end..data_end]);
            }
            b"IEND" if stage == Stage::ImageData && saw_image_data && length == 0 => {
                if chunk_end != bytes.len() {
                    return Err(PixelCompareError::TrailingData {
                        input,
                        trailing_bytes: bytes.len() - chunk_end,
                    });
                }
                let profile = profile.ok_or_else(|| malformed(input, PngDecodeStage::Structure))?;
                return Ok((profile, compressed_image_data));
            }
            _other => return Err(PixelCompareError::UnsupportedProfile { input }),
        }

        cursor = chunk_end;
        chunk_index = chunk_index.saturating_add(1);
        if cursor == bytes.len() {
            return Err(malformed(input, PngDecodeStage::Ending));
        }
    }
}

fn validate_exact_zlib_stream(
    input: PixelComparisonInput,
    profile: AcceptedPngProfile,
    compressed: &[u8],
) -> Result<(), PixelCompareError> {
    let expected_bytes = profile.inflated_scanline_bytes();
    let mut inflated = vec![0_u8; expected_bytes + 1];
    let mut decompressor = Decompress::new(true);
    let status = decompressor
        .decompress(compressed, &mut inflated, FlushDecompress::Finish)
        .map_err(|_error| malformed(input, PngDecodeStage::Pixels))?;
    if status != Status::StreamEnd
        || decompressor.total_in() != compressed.len() as u64
        || decompressor.total_out() != expected_bytes as u64
    {
        return Err(malformed(input, PngDecodeStage::Pixels));
    }
    Ok(())
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let polynomial = 0xedb8_8320 & 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ polynomial;
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_pixels() -> Vec<u8> {
        vec![0_u8; DECODED_RGBA_BYTES]
    }

    fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, width, height);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().expect("write PNG header");
            writer.write_image_data(pixels).expect("write PNG pixels");
            writer.finish().expect("finish PNG");
        }
        encoded
    }

    fn encode_rgb(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, width, height);
            encoder.set_color(ColorType::Rgb);
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().expect("write PNG header");
            writer.write_image_data(pixels).expect("write PNG pixels");
            writer.finish().expect("finish PNG");
        }
        encoded
    }

    fn mutate_ihdr_profile_byte(mut encoded: Vec<u8>, offset: usize, value: u8) -> Vec<u8> {
        assert!(matches!(offset, 24 | 25 | 28));
        encoded[offset] = value;
        let crc = png_crc32(&encoded[12..29]);
        encoded[29..33].copy_from_slice(&crc.to_be_bytes());
        encoded
    }

    fn append_to_last_idat(encoded: &[u8], suffix: &[u8]) -> Vec<u8> {
        let mut cursor = PNG_SIGNATURE.len();
        let mut last_idat = None;
        while cursor < encoded.len() {
            let length = usize::try_from(u32::from_be_bytes(
                encoded[cursor..cursor + 4]
                    .try_into()
                    .expect("PNG chunk length"),
            ))
            .expect("PNG chunk length fits usize");
            let header_end = cursor + 8;
            let data_end = header_end + length;
            let chunk_end = data_end + 4;
            if &encoded[cursor + 4..header_end] == b"IDAT" {
                last_idat = Some((cursor, header_end, data_end, chunk_end));
            }
            cursor = chunk_end;
        }
        let (chunk_start, data_start, data_end, chunk_end) =
            last_idat.expect("encoded PNG has IDAT");
        let new_length =
            u32::try_from(data_end - data_start + suffix.len()).expect("test IDAT length fits u32");
        let mut crc_input = Vec::with_capacity(4 + new_length as usize);
        crc_input.extend_from_slice(b"IDAT");
        crc_input.extend_from_slice(&encoded[data_start..data_end]);
        crc_input.extend_from_slice(suffix);

        let mut result = Vec::with_capacity(encoded.len() + suffix.len());
        result.extend_from_slice(&encoded[..chunk_start]);
        result.extend_from_slice(&new_length.to_be_bytes());
        result.extend_from_slice(&crc_input);
        result.extend_from_slice(&png_crc32(&crc_input).to_be_bytes());
        result.extend_from_slice(&encoded[chunk_end..]);
        result
    }

    fn compare_to_blank(actual_pixels: &[u8]) -> PixelComparison {
        let expected = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &blank_pixels());
        let actual = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, actual_pixels);
        compare_browser_pngs(&expected, &actual).expect("valid comparison")
    }

    #[test]
    fn identical_images_report_all_counts_and_are_gate_ineligible() {
        let pixels = blank_pixels();
        let comparison = compare_to_blank(&pixels);

        assert_eq!(comparison.format_version(), 1);
        assert_eq!(comparison.width(), 640);
        assert_eq!(comparison.height(), 480);
        assert_eq!(comparison.pixel_count(), 307_200);
        assert_eq!(comparison.channel_count(), 1_228_800);
        assert_eq!(comparison.changed_pixel_count(), 0);
        assert_eq!(comparison.changed_pixel_fraction(), 0.0);
        assert_eq!(comparison.mean_absolute_channel_delta(), 0.0);
        assert_eq!(comparison.max_observed_channel_delta(), 0);
        assert!(comparison.agrees());
        assert!(!comparison.gate_eligible());
    }

    #[test]
    fn channel_delta_eight_is_unchanged_and_nine_is_changed() {
        let mut pixels = blank_pixels();
        pixels[0] = 8;
        let at_boundary = compare_to_blank(&pixels);
        assert_eq!(at_boundary.changed_pixel_count(), 0);
        assert_eq!(at_boundary.max_observed_channel_delta(), 8);

        pixels[0] = 9;
        let above_boundary = compare_to_blank(&pixels);
        assert_eq!(above_boundary.changed_pixel_count(), 1);
        assert_eq!(above_boundary.max_observed_channel_delta(), 9);
    }

    #[test]
    fn changed_fraction_threshold_is_inclusive_and_exact() {
        let mut pixels = blank_pixels();
        let threshold_pixels = 6_144_usize;
        for pixel in pixels
            .as_chunks_mut::<CHANNELS_PER_PIXEL>()
            .0
            .iter_mut()
            .take(threshold_pixels)
        {
            pixel[0] = 9;
        }
        let at_boundary = compare_to_blank(&pixels);
        assert_eq!(at_boundary.changed_pixel_count(), 6_144);
        assert_eq!(at_boundary.changed_pixel_fraction(), 0.02);
        assert!(at_boundary.agrees());

        pixels[threshold_pixels * CHANNELS_PER_PIXEL] = 9;
        let above_boundary = compare_to_blank(&pixels);
        assert_eq!(above_boundary.changed_pixel_count(), 6_145);
        assert!(!above_boundary.agrees());
    }

    #[test]
    fn mean_delta_threshold_is_inclusive_and_exact() {
        let mut pixels = blank_pixels();
        for channel in &mut pixels[..602 * CHANNELS_PER_PIXEL] {
            *channel = 255;
        }
        let next = 602 * CHANNELS_PER_PIXEL;
        pixels[next] = 255;
        pixels[next + 1] = 105;

        let at_boundary = compare_to_blank(&pixels);
        assert_eq!(at_boundary.mean_absolute_channel_delta(), 0.5);
        assert!(at_boundary.agrees());

        pixels[next + 1] = 106;
        let above_boundary = compare_to_blank(&pixels);
        assert!(above_boundary.mean_absolute_channel_delta() > 0.5);
        assert!(!above_boundary.agrees());
    }

    #[test]
    fn alpha_is_compared_and_can_mark_a_pixel_changed() {
        let mut pixels = blank_pixels();
        pixels[3] = 9;
        let comparison = compare_to_blank(&pixels);

        assert_eq!(comparison.changed_pixel_count(), 1);
        assert_eq!(comparison.max_observed_channel_delta(), 9);
        assert_eq!(comparison.mean_absolute_channel_delta(), 9.0 / 1_228_800.0);
    }

    #[test]
    fn invalid_crc_is_rejected_even_in_final_chunk() {
        let mut encoded = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &blank_pixels());
        let final_byte = encoded.last_mut().expect("PNG has IEND CRC");
        *final_byte ^= 1;

        assert!(matches!(
            compare_browser_pngs(&encoded, &encoded),
            Err(PixelCompareError::ChecksumMismatch {
                input: PixelComparisonInput::Expected,
                ..
            })
        ));
    }

    #[test]
    fn rgb_and_equivalent_opaque_rgba_have_identical_metrics_in_both_orders() {
        let mut rgb_pixels = vec![0_u8; DECODED_RGB_BYTES];
        for (index, pixel) in rgb_pixels
            .as_chunks_mut::<RGB_CHANNELS_PER_PIXEL>()
            .0
            .iter_mut()
            .enumerate()
        {
            pixel.copy_from_slice(&[
                u8::try_from(index % 251).unwrap(),
                u8::try_from(index % 239).unwrap(),
                u8::try_from(index % 233).unwrap(),
            ]);
        }
        let mut rgba_pixels = Vec::with_capacity(DECODED_RGBA_BYTES);
        for pixel in rgb_pixels.as_chunks::<RGB_CHANNELS_PER_PIXEL>().0 {
            rgba_pixels.extend_from_slice(pixel);
            rgba_pixels.push(u8::MAX);
        }
        let rgb = encode_rgb(REQUIRED_WIDTH, REQUIRED_HEIGHT, &rgb_pixels);
        let rgba = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &rgba_pixels);

        for comparison in [
            compare_browser_pngs(&rgb, &rgba).unwrap(),
            compare_browser_pngs(&rgba, &rgb).unwrap(),
        ] {
            assert_eq!(comparison.changed_pixel_count(), 0);
            assert_eq!(comparison.mean_absolute_channel_delta(), 0.0);
            assert_eq!(comparison.max_observed_channel_delta(), 0);
            assert!(comparison.agrees());
            assert!(!comparison.gate_eligible());
        }
    }

    #[test]
    fn other_color_types_depths_and_interlace_are_rejected() {
        let rgba = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &blank_pixels());
        for unsupported in [
            mutate_ihdr_profile_byte(rgba.clone(), 25, 0),
            mutate_ihdr_profile_byte(rgba.clone(), 24, 16),
            mutate_ihdr_profile_byte(rgba.clone(), 28, 1),
        ] {
            assert_eq!(
                compare_browser_pngs(&unsupported, &rgba),
                Err(PixelCompareError::UnsupportedProfile {
                    input: PixelComparisonInput::Expected,
                })
            );
        }
    }

    #[test]
    fn crc_correct_bytes_after_the_zlib_stream_inside_idat_are_rejected() {
        let valid = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &blank_pixels());
        let suffixed = append_to_last_idat(&valid, &[0]);

        assert_eq!(
            compare_browser_pngs(&suffixed, &valid),
            Err(PixelCompareError::MalformedPng {
                input: PixelComparisonInput::Expected,
                stage: PngDecodeStage::Pixels,
            })
        );
    }

    #[test]
    fn wrong_dimensions_are_rejected_on_each_side() {
        let wrong = encode_rgba(639, REQUIRED_HEIGHT, &vec![0_u8; 639 * 480 * 4]);
        let valid = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &blank_pixels());

        assert!(matches!(
            compare_browser_pngs(&wrong, &valid),
            Err(PixelCompareError::Dimensions {
                input: PixelComparisonInput::Expected,
                width: 639,
                height: 480,
                ..
            })
        ));
        assert!(matches!(
            compare_browser_pngs(&valid, &wrong),
            Err(PixelCompareError::Dimensions {
                input: PixelComparisonInput::Actual,
                width: 639,
                height: 480,
                ..
            })
        ));
    }

    #[test]
    fn bytes_after_iend_are_rejected_as_trailing_data() {
        let mut encoded = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &blank_pixels());
        encoded.extend_from_slice(b"tail");

        assert_eq!(
            compare_browser_pngs(&encoded, &encoded),
            Err(PixelCompareError::TrailingData {
                input: PixelComparisonInput::Expected,
                trailing_bytes: 4,
            })
        );
    }

    #[test]
    fn truncated_png_is_typed_as_malformed() {
        let mut encoded = encode_rgba(REQUIRED_WIDTH, REQUIRED_HEIGHT, &blank_pixels());
        encoded.truncate(encoded.len() - 2);

        assert!(matches!(
            compare_browser_pngs(&encoded, &encoded),
            Err(PixelCompareError::MalformedPng {
                input: PixelComparisonInput::Expected,
                stage: PngDecodeStage::Structure,
            })
        ));
    }

    #[test]
    fn encoded_limit_is_enforced_before_png_parsing() {
        let oversized = vec![0_u8; MAX_ENCODED_PNG_BYTES + 1];

        assert_eq!(
            compare_browser_pngs(&oversized, &oversized),
            Err(PixelCompareError::EncodedByteLimit {
                input: PixelComparisonInput::Expected,
                encoded_bytes: MAX_ENCODED_PNG_BYTES + 1,
                max_encoded_bytes: MAX_ENCODED_PNG_BYTES,
            })
        );
    }
}
