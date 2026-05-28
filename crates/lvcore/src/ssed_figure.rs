use crate::error::{Error, Result};
use crate::image::encode_png_rgba;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FigureDimensions {
    pub width: u32,
    pub height: u32,
}

impl FigureDimensions {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::Driver(
                "FIGURE dimensions must be positive".to_owned(),
            ));
        }
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or_else(|| Error::Driver("FIGURE dimensions overflow".to_owned()))?;
        if pixels > 10_000_000 {
            return Err(Error::Driver(format!(
                "FIGURE dimensions are too large: {width}x{height}"
            )));
        }
        Ok(Self { width, height })
    }

    pub fn row_stride(self) -> Result<usize> {
        let width = usize::try_from(self.width)
            .map_err(|_| Error::Driver("FIGURE width does not fit usize".to_owned()))?;
        width
            .checked_add(7)
            .map(|value| value / 8)
            .ok_or_else(|| Error::Driver("FIGURE row stride overflow".to_owned()))
    }

    pub fn bitmap_bytes(self) -> Result<usize> {
        let height = usize::try_from(self.height)
            .map_err(|_| Error::Driver("FIGURE height does not fit usize".to_owned()))?;
        self.row_stride()?
            .checked_mul(height)
            .ok_or_else(|| Error::Driver("FIGURE bitmap byte length overflow".to_owned()))
    }
}

pub fn parse_figure_dimensions(payload: &[u8]) -> Option<FigureDimensions> {
    if payload.len() < 10 {
        return None;
    }
    let height = decode_bcd_decimal(&payload[2..6])?;
    let width = decode_bcd_decimal(&payload[6..10])?;
    FigureDimensions::new(width, height).ok()
}

pub fn parse_figure_pointer(payload: &[u8]) -> Option<(u32, u32)> {
    if payload.len() < 6 {
        return None;
    }
    Some((
        decode_bcd_decimal(&payload[..4])?,
        decode_bcd_decimal(&payload[4..6])?,
    ))
}

pub fn figure_bitmap_to_png(bitmap: &[u8], dimensions: FigureDimensions) -> Result<Vec<u8>> {
    let expected = dimensions.bitmap_bytes()?;
    if bitmap.len() != expected {
        return Err(Error::Driver(format!(
            "FIGURE bitmap has {} bytes; expected {expected}",
            bitmap.len()
        )));
    }
    encode_png_rgba(
        dimensions.width,
        dimensions.height,
        &figure_bitmap_to_rgba(bitmap, dimensions)?,
    )
}

fn figure_bitmap_to_rgba(bitmap: &[u8], dimensions: FigureDimensions) -> Result<Vec<u8>> {
    let width = usize::try_from(dimensions.width)
        .map_err(|_| Error::Driver("FIGURE width does not fit usize".to_owned()))?;
    let height = usize::try_from(dimensions.height)
        .map_err(|_| Error::Driver("FIGURE height does not fit usize".to_owned()))?;
    let stride = dimensions.row_stride()?;
    let mut pixels = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row = &bitmap[y * stride..(y + 1) * stride];
        for x in 0..width {
            let byte = row[x / 8];
            let on = byte & (0x80 >> (x % 8)) != 0;
            if on {
                pixels.extend_from_slice(&[0, 0, 0, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Ok(pixels)
}

fn decode_bcd_decimal(data: &[u8]) -> Option<u32> {
    let mut value = 0_u32;
    for byte in data {
        let high = byte >> 4;
        let low = byte & 0x0f;
        if high > 9 || low > 9 {
            return None;
        }
        value = value.checked_mul(100)?;
        value = value.checked_add(u32::from(high) * 10 + u32::from(low))?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_figure_descriptor_and_pointer() {
        let dimensions =
            parse_figure_dimensions(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x09])
                .unwrap();

        assert_eq!(dimensions.width, 9);
        assert_eq!(dimensions.height, 2);
        assert_eq!(dimensions.row_stride().unwrap(), 2);
        assert_eq!(dimensions.bitmap_bytes().unwrap(), 4);
        assert_eq!(
            parse_figure_pointer(&[0x00, 0x00, 0x12, 0x00, 0x00, 0x17]),
            Some((1200, 17))
        );
    }

    #[test]
    fn converts_variable_figure_bitmap_to_png() {
        let dimensions = FigureDimensions::new(9, 2).unwrap();
        let png = figure_bitmap_to_png(&[0x80, 0x80, 0x7f, 0x00], dimensions).unwrap();

        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
