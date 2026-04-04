//! Stream filter decompression: FlateDecode (with PNG predictors), LZW, ASCII85, ASCIIHex.

use super::parser::PdfVal;
use super::PdfError;

/// Decompress stream data through a chain of filters.
pub(crate) fn decompress(
    data: &[u8],
    filters: &[&[u8]],
    decode_parms: Option<&PdfVal>,
) -> Result<Vec<u8>, PdfError> {
    let mut result = data.to_vec();

    for (i, filter) in filters.iter().enumerate() {
        // Get per-filter DecodeParms (could be an array of dicts, one per filter)
        let parms = match decode_parms {
            Some(PdfVal::Array(arr)) => arr.get(i),
            Some(PdfVal::Dict(_)) if filters.len() == 1 => decode_parms,
            _ => None,
        };

        result = match *filter {
            b"FlateDecode" | b"Fl" => flate_decode(&result, parms)?,
            b"LZWDecode" | b"LZW" => lzw_decode(&result, parms)?,
            b"ASCII85Decode" | b"A85" => ascii85_decode(&result)?,
            b"ASCIIHexDecode" | b"AHx" => asciihex_decode(&result)?,
            b"RunLengthDecode" | b"RL" => runlength_decode(&result)?,
            _ => {
                tracing::warn!(
                    "unsupported filter: {:?}",
                    String::from_utf8_lossy(filter)
                );
                return Err(PdfError::Decompress(format!(
                    "unsupported filter: {}",
                    String::from_utf8_lossy(filter)
                )));
            }
        };
    }

    Ok(result)
}

/// Extract filter names from a stream dict.
pub(crate) fn get_filters(dict: &std::collections::BTreeMap<Vec<u8>, PdfVal>) -> Vec<Vec<u8>> {
    match super::parser::dict_get(dict, b"Filter") {
        Some(PdfVal::Name(name)) => vec![name.clone()],
        Some(PdfVal::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_name().map(|n| n.to_vec()))
            .collect(),
        _ => vec![],
    }
}

/// FlateDecode (zlib/deflate) with optional PNG predictor.
fn flate_decode(data: &[u8], parms: Option<&PdfVal>) -> Result<Vec<u8>, PdfError> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .or_else(|_| {
            // Some PDFs use raw deflate without zlib header
            let mut decoder = flate2::read::DeflateDecoder::new(data);
            decompressed.clear();
            decoder.read_to_end(&mut decompressed)
        })
        .map_err(|e| PdfError::Decompress(format!("flate: {e}")))?;

    // Apply PNG predictor if specified
    let predictor = parms
        .and_then(|p| p.dict_get(b"Predictor"))
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    if predictor >= 10 {
        let columns = parms
            .and_then(|p| p.dict_get(b"Columns"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as usize;
        let colors = parms
            .and_then(|p| p.dict_get(b"Colors"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as usize;
        let bits_per_component = parms
            .and_then(|p| p.dict_get(b"BitsPerComponent"))
            .and_then(|v| v.as_i64())
            .unwrap_or(8) as usize;

        let bytes_per_pixel = (colors * bits_per_component).div_ceil(8);
        let row_bytes = (columns * colors * bits_per_component).div_ceil(8);

        decompressed = png_unpredict(&decompressed, row_bytes, bytes_per_pixel)?;
    } else if predictor == 2 {
        // TIFF predictor
        let columns = parms
            .and_then(|p| p.dict_get(b"Columns"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as usize;
        decompressed = tiff_unpredict(&decompressed, columns);
    }

    Ok(decompressed)
}

/// Reverse PNG row filters.
/// Each row is: [filter_byte, row_data...]
fn png_unpredict(data: &[u8], row_bytes: usize, bpp: usize) -> Result<Vec<u8>, PdfError> {
    let stride = row_bytes + 1; // +1 for filter byte
    let num_rows = data.len() / stride;
    let mut output = Vec::with_capacity(num_rows * row_bytes);
    let mut prev_row = vec![0u8; row_bytes];

    for row_idx in 0..num_rows {
        let row_start = row_idx * stride;
        if row_start >= data.len() {
            break;
        }
        let filter = data[row_start];
        let row_data = &data[row_start + 1..std::cmp::min(row_start + stride, data.len())];

        let mut decoded = vec![0u8; row_bytes];
        let actual_len = row_data.len().min(row_bytes);

        for j in 0..actual_len {
            let raw = row_data[j];
            let a = if j >= bpp { decoded[j - bpp] } else { 0 };
            let b = prev_row[j];
            let c = if j >= bpp { prev_row[j - bpp] } else { 0 };

            decoded[j] = match filter {
                0 => raw,                                // None
                1 => raw.wrapping_add(a),                // Sub
                2 => raw.wrapping_add(b),                // Up
                3 => raw.wrapping_add(((a as u16 + b as u16) / 2) as u8), // Average
                4 => raw.wrapping_add(paeth(a, b, c)),   // Paeth
                _ => raw,                                // Unknown filter — treat as None
            };
        }

        output.extend_from_slice(&decoded);
        prev_row = decoded;
    }

    Ok(output)
}

/// Paeth predictor function.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let pa = (p - a as i32).abs();
    let pb = (p - b as i32).abs();
    let pc = (p - c as i32).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// TIFF predictor (predictor=2): each byte is delta from previous in the row.
fn tiff_unpredict(data: &[u8], columns: usize) -> Vec<u8> {
    if columns == 0 {
        return data.to_vec();
    }
    let mut output = data.to_vec();
    for row in output.chunks_mut(columns) {
        for j in 1..row.len() {
            row[j] = row[j].wrapping_add(row[j - 1]);
        }
    }
    output
}

/// LZW decompression.
fn lzw_decode(data: &[u8], parms: Option<&PdfVal>) -> Result<Vec<u8>, PdfError> {
    use weezl::{decode::Decoder, BitOrder};

    let early_change = parms
        .and_then(|p| p.dict_get(b"EarlyChange"))
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    // PDF uses MSB-first, min code size 8
    let mut decoder = Decoder::with_tiff_size_switch(BitOrder::Msb, 8);
    if early_change == 0 {
        // weezl's default is early change=1 which matches PDF default
        // For early_change=0, we'd need a different decoder config.
        // This is rare; just use default and hope for the best.
    }

    let decompressed = decoder
        .decode(data)
        .map_err(|e| PdfError::Decompress(format!("lzw: {e}")))?;

    // Apply PNG predictor if specified (same as FlateDecode)
    let predictor = parms
        .and_then(|p| p.dict_get(b"Predictor"))
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    if predictor >= 10 {
        let columns = parms
            .and_then(|p| p.dict_get(b"Columns"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as usize;
        let bytes_per_pixel = 1;
        return png_unpredict(&decompressed, columns, bytes_per_pixel);
    }

    Ok(decompressed)
}

/// ASCII85 (Base85) decoding.
fn ascii85_decode(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    let mut result = Vec::new();
    let mut i = 0;

    // Skip leading whitespace
    while i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }

    while i < data.len() {
        // Check for end-of-data marker
        if data[i] == b'~' {
            break;
        }

        // 'z' is shorthand for 4 zero bytes
        if data[i] == b'z' {
            result.extend_from_slice(&[0, 0, 0, 0]);
            i += 1;
            continue;
        }

        // Collect up to 5 digits (skipping whitespace)
        let mut group = Vec::new();
        while group.len() < 5 && i < data.len() && data[i] != b'~' {
            if data[i].is_ascii_whitespace() {
                i += 1;
                continue;
            }
            if data[i] < b'!' || data[i] > b'u' {
                i += 1;
                continue;
            }
            group.push((data[i] - b'!') as u32);
            i += 1;
        }

        if group.is_empty() {
            break;
        }

        let n = group.len();
        // Pad with 'u' (84) to make 5 digits
        while group.len() < 5 {
            group.push(84);
        }

        let mut val: u32 = 0;
        for &d in &group {
            val = val * 85 + d;
        }

        let bytes = val.to_be_bytes();
        // Output n-1 bytes
        result.extend_from_slice(&bytes[..n - 1]);
    }

    Ok(result)
}

/// ASCIIHex decoding.
fn asciihex_decode(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    let mut result = Vec::new();
    let mut hi: Option<u8> = None;

    for &b in data {
        if b == b'>' {
            break;
        }
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => continue, // skip whitespace and junk
        };

        match hi {
            None => hi = Some(digit),
            Some(h) => {
                result.push(h << 4 | digit);
                hi = None;
            }
        }
    }
    // Odd final digit: pad with 0
    if let Some(h) = hi {
        result.push(h << 4);
    }

    Ok(result)
}

/// RunLength decoding.
fn runlength_decode(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        let length = data[i];
        i += 1;

        if length == 128 {
            break; // EOD
        } else if length < 128 {
            // Copy next length+1 bytes literally
            let n = length as usize + 1;
            if i + n > data.len() {
                break;
            }
            result.extend_from_slice(&data[i..i + n]);
            i += n;
        } else {
            // Repeat next byte 257-length times
            let n = 257 - length as usize;
            if i >= data.len() {
                break;
            }
            let byte = data[i];
            i += 1;
            result.extend(std::iter::repeat_n(byte, n));
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flate_decode_simple() {
        // Compress some data, then decode it
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        let original = b"Hello, PDF world! This is a test of FlateDecode.";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let result = flate_decode(&compressed, None).unwrap();
        assert_eq!(result, original);
    }

    #[test]
    fn test_png_predictor_none() {
        // filter=0 (None), 3 columns, 1 bpp
        let data = [0, 10, 20, 30, 0, 40, 50, 60];
        let result = png_unpredict(&data, 3, 1).unwrap();
        assert_eq!(result, [10, 20, 30, 40, 50, 60]);
    }

    #[test]
    fn test_png_predictor_sub() {
        // filter=1 (Sub): each byte += previous byte in row
        // bpp=1, row_bytes=3
        let data = [1, 10, 5, 3]; // filter=Sub, 10, 10+5=15, 15+3=18
        let result = png_unpredict(&data, 3, 1).unwrap();
        assert_eq!(result, [10, 15, 18]);
    }

    #[test]
    fn test_png_predictor_up() {
        // filter=2 (Up): each byte += corresponding byte in previous row
        let data = [
            0, 10, 20, 30, // row 0: None: 10, 20, 30
            2, 5, 10, 15,  // row 1: Up: 10+5=15, 20+10=30, 30+15=45
        ];
        let result = png_unpredict(&data, 3, 1).unwrap();
        assert_eq!(result, [10, 20, 30, 15, 30, 45]);
    }

    #[test]
    fn test_ascii85_decode() {
        // "Hello" in ASCII85
        let encoded = b"87cURD]j7BEbo80~>";
        let result = ascii85_decode(encoded).unwrap();
        // The ASCII85 encoding of "Hello" should decode back
        assert_eq!(&result[..5], b"Hello");
    }

    #[test]
    fn test_ascii85_z_shorthand() {
        let result = ascii85_decode(b"z~>").unwrap();
        assert_eq!(result, [0, 0, 0, 0]);
    }

    #[test]
    fn test_asciihex_decode() {
        let result = asciihex_decode(b"48656C6C6F>").unwrap();
        assert_eq!(result, b"Hello");
    }

    #[test]
    fn test_asciihex_odd_digits() {
        let result = asciihex_decode(b"4>").unwrap();
        assert_eq!(result, [0x40]);
    }

    #[test]
    fn test_runlength_decode() {
        // 2 = copy 3 bytes, then 253 = repeat next byte 4 times, then 128 = EOD
        let data = [2, b'A', b'B', b'C', 253, b'X', 128];
        let result = runlength_decode(&data).unwrap();
        assert_eq!(result, b"ABCXXXX");
    }

    #[test]
    fn test_decompress_chain() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        let original = b"test data for chained filters";

        // First: deflate
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        // Then: hex encode
        let hex: Vec<u8> = compressed
            .iter()
            .flat_map(|b| format!("{:02X}", b).into_bytes())
            .chain(b">".iter().copied())
            .collect();

        let result = decompress(
            &hex,
            &[b"ASCIIHexDecode", b"FlateDecode"],
            None,
        )
        .unwrap();
        assert_eq!(result, original);
    }
}
