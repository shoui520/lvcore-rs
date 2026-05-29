use crate::error::{Error, Result};
use crate::image::encode_png_rgba;

const GA16_DATA_OFFSET: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ga16Resource {
    width: usize,
    height: usize,
    start_code: u16,
    count: usize,
    glyph_bytes: usize,
}

pub fn ga16_resource_covers_code(data: &[u8], code: &str) -> bool {
    let Some((resource, code)) = parse_resource_and_code(data, code) else {
        return false;
    };
    resource.glyph_index_for_code(code).is_some_and(|index| {
        index < resource.count
            && GA16_DATA_OFFSET
                .checked_add((index + 1).saturating_mul(resource.glyph_bytes))
                .is_some_and(|end| end <= data.len())
    })
}

pub fn ga16_glyph_png(data: &[u8], code: &str) -> Result<Vec<u8>> {
    let (resource, code) = parse_resource_and_code(data, code)
        .ok_or_else(|| Error::Driver(format!("invalid GA16 glyph reference: {code}")))?;
    let index = resource
        .glyph_index_for_code(code)
        .filter(|index| *index < resource.count)
        .ok_or_else(|| Error::Driver(format!("GA16 glyph {code:04X} is outside resource range")))?;
    let start = GA16_DATA_OFFSET + index * resource.glyph_bytes;
    let end = start + resource.glyph_bytes;
    let glyph = data
        .get(start..end)
        .ok_or_else(|| Error::Driver(format!("GA16 glyph {code:04X} is truncated")))?;
    let rgba = render_ga16_glyph_rgba(glyph, resource.width, resource.height)?;
    encode_png_rgba(resource.width as u32, resource.height as u32, &rgba)
}

fn parse_resource_and_code(data: &[u8], code: &str) -> Option<(Ga16Resource, u16)> {
    let resource = parse_ga16_resource(data)?;
    let code = u16::from_str_radix(code, 16).ok()?;
    Some((resource, code))
}

fn parse_ga16_resource(data: &[u8]) -> Option<Ga16Resource> {
    if data.len() < 16 {
        return None;
    }
    let width = usize::from(data[8]);
    let height = usize::from(data[9]);
    if width == 0 || height == 0 {
        return None;
    }
    let start_code = u16::from_be_bytes([data[10], data[11]]);
    let count = usize::from(u16::from_be_bytes([data[12], data[13]]));
    let glyph_bytes = ga16_row_size(width).checked_mul(height)?;
    Some(Ga16Resource {
        width,
        height,
        start_code,
        count,
        glyph_bytes,
    })
}

impl Ga16Resource {
    fn glyph_index_for_code(&self, code: u16) -> Option<usize> {
        let start_row = i32::from((self.start_code >> 8) & 0xff);
        let start_cell = i32::from(self.start_code & 0xff);
        let row = i32::from((code >> 8) & 0xff);
        let cell = i32::from(code & 0xff);
        let index = if (0x21..=0x7e).contains(&start_cell) && (0x21..=0x7e).contains(&cell) {
            ((row - start_row) * 0x5e) + (cell - start_cell)
        } else {
            i32::from(code) - i32::from(self.start_code)
        };
        usize::try_from(index).ok()
    }
}

fn ga16_row_size(width: usize) -> usize {
    width.div_ceil(8)
}

fn render_ga16_glyph_rgba(glyph: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let row_size = ga16_row_size(width);
    let required = row_size
        .checked_mul(height)
        .ok_or_else(|| Error::Driver("GA16 glyph dimensions overflow".to_owned()))?;
    if glyph.len() < required {
        return Err(Error::Driver(format!(
            "GA16 glyph has {} bytes; expected {required}",
            glyph.len()
        )));
    }

    let mut pixels = vec![0_u8; width * height * 4];
    let mut output = 0usize;
    for y in 0..height {
        let row = &glyph[y * row_size..(y + 1) * row_size];
        for x in 0..width {
            let byte = row[x / 8];
            let bit = 0x80 >> (x % 8);
            if byte & bit != 0 {
                pixels[output..output + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
            output += 4;
        }
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ga16() -> Vec<u8> {
        let mut data = vec![0_u8; GA16_DATA_OFFSET + 2];
        data[8] = 8;
        data[9] = 2;
        data[10..12].copy_from_slice(&0xB121_u16.to_be_bytes());
        data[12..14].copy_from_slice(&1_u16.to_be_bytes());
        data[GA16_DATA_OFFSET] = 0b1000_0001;
        data[GA16_DATA_OFFSET + 1] = 0b0100_0010;
        data
    }

    #[test]
    fn detects_ga16_direct_grid_coverage() {
        let data = sample_ga16();
        assert!(ga16_resource_covers_code(&data, "B121"));
        assert!(!ga16_resource_covers_code(&data, "B122"));

        let truncated = data[..GA16_DATA_OFFSET + 1].to_vec();
        assert!(!ga16_resource_covers_code(&truncated, "B121"));
    }

    #[test]
    fn renders_ga16_glyph_as_png() {
        let data = sample_ga16();
        let png = ga16_glyph_png(&data, "B121").unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
