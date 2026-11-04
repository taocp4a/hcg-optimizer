//! Image subfolder file-name ID (HCG_FORMAT.md §6.3, reference `image_id()`).
//!
//! ```text
//! s         = IntToHex(stamp, 4) + LowerCaseASCII(absolute_path)
//! ID        = CRC32(UTF-16LE(s), init = 0x47434448 'HDCG')
//! file name = '%.8X' % ID + file_name_part(path)
//! ```

/// CRC seed: ASCII `HDCG` (little-endian u32).
const CRC_SEED: u32 = 0x4743_4448;

/// Computes the 8-digit upper-case hex ID from a path and the original file's modification stamp.
///
/// Only ASCII `A`–`Z` are lower-cased; all other characters (e.g. Hangul) are kept as is.
pub fn image_id(path: &str, stamp: u64) -> String {
    let lowered: String = path.chars().map(|c| c.to_ascii_lowercase()).collect();
    let s = format!("{stamp:04X}{lowered}");
    let mut hasher = crc32fast::Hasher::new_with_initial(CRC_SEED);
    for unit in s.encode_utf16() {
        hasher.update(&unit.to_le_bytes());
    }
    format!("{:08X}", hasher.finalize())
}

/// Builds the file name inside the image folder (`ID + file name`). Accepts both `\` and `/` as separators.
pub fn subfolder_file_name(path: &str, stamp: u64) -> String {
    let base = path.rsplit(['\\', '/']).next().unwrap_or(path);
    format!("{}{}", image_id(path, stamp), base)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values: real image-folder file names in project/samples (match reference/hcg_parse.py).
    #[test]
    fn matches_sample_folder_names() {
        let cases = [
            (
                r"C:\test\mark_1.png",
                3_177_945_114_533_072,
                "28BD2D7Emark_1.png",
            ),
            (
                r"C:\test\bg_6000x4000_photo.jpg",
                3_177_940_876_203_604,
                "D727E722bg_6000x4000_photo.jpg",
            ),
            (
                r"C:\test\bg_1080_small.jpg",
                3_177_940_876_203_592,
                "D4E3A7EAbg_1080_small.jpg",
            ),
            (
                r"C:\test\bg_1080_noise.png",
                3_177_940_876_203_592,
                "2D00575Abg_1080_noise.png",
            ),
        ];
        for (path, stamp, expected) in cases {
            assert_eq!(subfolder_file_name(path, stamp), expected, "{path}");
        }
    }

    #[test]
    fn lowercase_is_ascii_only() {
        assert_eq!(
            image_id(r"C:\TEST\Mark_1.PNG", 1),
            image_id(r"c:\test\mark_1.png", 1)
        );
        assert_ne!(image_id("Ä", 1), image_id("ä", 1));
    }
}
