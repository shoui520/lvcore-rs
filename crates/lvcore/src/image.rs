use crate::error::{Error, Result};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub(crate) fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let width_usize = usize::try_from(width)
        .map_err(|_| Error::Driver("PNG width does not fit usize".to_owned()))?;
    let height_usize = usize::try_from(height)
        .map_err(|_| Error::Driver("PNG height does not fit usize".to_owned()))?;
    let row_bytes = width_usize
        .checked_mul(4)
        .ok_or_else(|| Error::Driver("PNG row byte length overflow".to_owned()))?;
    let expected = row_bytes
        .checked_mul(height_usize)
        .ok_or_else(|| Error::Driver("PNG RGBA byte length overflow".to_owned()))?;
    if rgba.len() != expected {
        return Err(Error::Driver(format!(
            "RGBA buffer has {} bytes; expected {expected}",
            rgba.len()
        )));
    }

    let mut scanlines = Vec::with_capacity(expected + height_usize);
    for row in rgba.chunks_exact(row_bytes) {
        scanlines.push(0);
        scanlines.extend_from_slice(row);
    }

    let mut png = Vec::new();
    png.extend_from_slice(PNG_SIGNATURE);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(6);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    write_png_chunk(&mut png, b"IHDR", &ihdr);
    write_png_chunk(&mut png, b"IDAT", &zlib_store_blocks(&scanlines));
    write_png_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

fn zlib_store_blocks(data: &[u8]) -> Vec<u8> {
    let mut zlib = Vec::with_capacity(data.len() + data.len() / 65_535 * 5 + 8);
    zlib.extend_from_slice(&[0x78, 0x01]);
    let mut remaining = data;
    while !remaining.is_empty() {
        let take = remaining.len().min(65_535);
        let final_block = take == remaining.len();
        zlib.push(u8::from(final_block));
        let len = take as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(&remaining[..take]);
        remaining = &remaining[take..];
    }
    if data.is_empty() {
        zlib.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    zlib.extend_from_slice(&adler32(data).to_be_bytes());
    zlib
}

fn write_png_chunk(output: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(tag);
    output.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(tag.len() + data.len());
    crc_data.extend_from_slice(tag);
    crc_data.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut a = 1_u32;
    let mut b = 0_u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_rgba_png_signature_and_chunks() {
        let png = encode_png_rgba(1, 1, &[0, 0, 0, 255]).unwrap();

        assert!(png.starts_with(PNG_SIGNATURE));
        assert!(png.windows(4).any(|window| window == b"IHDR"));
        assert!(png.windows(4).any(|window| window == b"IDAT"));
        assert!(png.windows(4).any(|window| window == b"IEND"));
    }
}
