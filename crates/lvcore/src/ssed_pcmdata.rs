use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmDataMediaKind {
    Wave,
    Mp3,
}

impl PcmDataMediaKind {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Wave => "audio/wav",
            Self::Mp3 => "audio/mpeg",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmDataParseResult {
    pub media_kind: PcmDataMediaKind,
    pub content_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaveFormat {
    format_tag: u16,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SharedWaveStream {
    fmt_offset: usize,
    data_offset: usize,
    data_size: usize,
    format: WaveFormat,
}

impl SharedWaveStream {
    fn contains(self, relative_offset: usize, size: usize) -> bool {
        let Some(data_end) = self.data_offset.checked_add(self.data_size) else {
            return false;
        };
        relative_offset >= self.data_offset
            && size > 0
            && relative_offset
                .checked_add(size)
                .is_some_and(|end| end <= data_end)
    }
}

pub fn pcmdata_portable_audio_bytes(
    relative_offset: usize,
    raw: &[u8],
    component_prefix: &[u8],
) -> Result<(Vec<u8>, PcmDataParseResult)> {
    if raw.starts_with(b"fmt ") {
        return parse_wave_chunks(raw);
    }
    if raw.starts_with(b"ID3") || mp3_frame_sync(raw) {
        let content_size = raw.len().saturating_sub(trailing_zero_count(raw, 0));
        return Ok((
            raw[..content_size].to_vec(),
            PcmDataParseResult {
                media_kind: PcmDataMediaKind::Mp3,
                content_size,
            },
        ));
    }
    if let Some(shared) = detect_shared_wave_stream(component_prefix)
        && shared.contains(relative_offset, raw.len())
    {
        let media_kind = if shared.format.format_tag == 0x0055 {
            PcmDataMediaKind::Mp3
        } else {
            PcmDataMediaKind::Wave
        };
        if media_kind == PcmDataMediaKind::Mp3 {
            return Ok((
                raw.to_vec(),
                PcmDataParseResult {
                    media_kind,
                    content_size: raw.len(),
                },
            ));
        }
        let wave = make_riff_wave(&wave_chunks_for_slice(shared.format, raw));
        return Ok((
            wave,
            PcmDataParseResult {
                media_kind,
                content_size: raw.len(),
            },
        ));
    }
    Err(Error::Driver(
        "PCMDATA range did not decode as WAVE chunks, MP3 bytes, or shared WAVE slice".to_owned(),
    ))
}

pub fn pcmdata_audio_summary(
    relative_offset: usize,
    raw: &[u8],
    component_prefix: &[u8],
) -> Result<PcmDataParseResult> {
    if raw.starts_with(b"fmt ") {
        return parse_wave_chunks_summary(raw);
    }
    if raw.starts_with(b"ID3") || mp3_frame_sync(raw) {
        let content_size = raw.len().saturating_sub(trailing_zero_count(raw, 0));
        return Ok(PcmDataParseResult {
            media_kind: PcmDataMediaKind::Mp3,
            content_size,
        });
    }
    if let Some(shared) = detect_shared_wave_stream(component_prefix)
        && shared.contains(relative_offset, raw.len())
    {
        let media_kind = if shared.format.format_tag == 0x0055 {
            PcmDataMediaKind::Mp3
        } else {
            PcmDataMediaKind::Wave
        };
        return Ok(PcmDataParseResult {
            media_kind,
            content_size: raw.len(),
        });
    }
    Err(Error::Driver(
        "PCMDATA range did not decode as WAVE chunks, MP3 bytes, or shared WAVE slice".to_owned(),
    ))
}

fn parse_wave_chunks_summary(raw: &[u8]) -> Result<PcmDataParseResult> {
    let (chunks, format, data) = parse_wave_chunks_metadata(raw)?;
    if format.format_tag == 0x0055 {
        return Ok(PcmDataParseResult {
            media_kind: PcmDataMediaKind::Mp3,
            content_size: data.size,
        });
    }
    let content_size = chunks
        .last()
        .map(|chunk| chunk.padded_end)
        .unwrap_or(raw.len())
        .min(raw.len());
    Ok(PcmDataParseResult {
        media_kind: PcmDataMediaKind::Wave,
        content_size,
    })
}

fn parse_wave_chunks_metadata(raw: &[u8]) -> Result<(Vec<RiffChunk>, WaveFormat, RiffChunk)> {
    let chunks = parse_riff_chunks(raw)
        .ok_or_else(|| Error::Driver("PCMDATA WAVE chunk sequence did not parse".to_owned()))?;
    let fmt_index = chunks
        .iter()
        .position(|chunk| chunk.tag == *b"fmt ")
        .ok_or_else(|| Error::Driver("PCMDATA WAVE chunks are missing fmt".to_owned()))?;
    let data_index = chunks
        .iter()
        .position(|chunk| chunk.tag == *b"data")
        .ok_or_else(|| Error::Driver("PCMDATA WAVE chunks are missing data".to_owned()))?;
    let fmt = chunks[fmt_index];
    let data = chunks[data_index];
    let format = parse_wave_format(&raw[fmt.payload_range()])
        .ok_or_else(|| Error::Driver("PCMDATA WAVE fmt chunk did not decode".to_owned()))?;
    Ok((chunks, format, data))
}

fn parse_wave_chunks(raw: &[u8]) -> Result<(Vec<u8>, PcmDataParseResult)> {
    let (chunks, format, data) = parse_wave_chunks_metadata(raw)?;
    if format.format_tag == 0x0055 {
        let bytes = raw[data.payload_range()].to_vec();
        return Ok((
            bytes,
            PcmDataParseResult {
                media_kind: PcmDataMediaKind::Mp3,
                content_size: data.size,
            },
        ));
    }
    let content_size = chunks
        .last()
        .map(|chunk| chunk.padded_end)
        .unwrap_or(raw.len())
        .min(raw.len());
    Ok((
        make_riff_wave(&raw[..content_size]),
        PcmDataParseResult {
            media_kind: PcmDataMediaKind::Wave,
            content_size,
        },
    ))
}

#[derive(Debug, Clone, Copy)]
struct RiffChunk {
    tag: [u8; 4],
    size: usize,
    payload_offset: usize,
    end: usize,
    padded_end: usize,
}

impl RiffChunk {
    fn payload_range(self) -> std::ops::Range<usize> {
        self.payload_offset..self.end
    }
}

fn parse_riff_chunks(raw: &[u8]) -> Option<Vec<RiffChunk>> {
    let mut pos = 0;
    let mut chunks = Vec::new();
    while pos + 8 <= raw.len() {
        let tag: [u8; 4] = raw[pos..pos + 4].try_into().ok()?;
        if !is_ascii_chunk_tag(&tag) {
            break;
        }
        let size = u32::from_le_bytes(raw[pos + 4..pos + 8].try_into().ok()?) as usize;
        let payload_offset = pos + 8;
        let end = payload_offset.checked_add(size)?;
        let padded_end = end.checked_add(size & 1)?;
        if end > raw.len() {
            return None;
        }
        chunks.push(RiffChunk {
            tag,
            size,
            payload_offset,
            end,
            padded_end,
        });
        pos = padded_end;
        if tag == *b"data" {
            break;
        }
    }
    (!chunks.is_empty()).then_some(chunks)
}

fn detect_shared_wave_stream(prefix: &[u8]) -> Option<SharedWaveStream> {
    let search_limit = prefix.len().min(2048);
    let mut pos = 0;
    while pos + 8 <= search_limit {
        let fmt_offset = find_bytes(&prefix[pos..search_limit], b"fmt ")?;
        let fmt_offset = pos + fmt_offset;
        let fmt_size = u32::from_le_bytes(
            prefix
                .get(fmt_offset + 4..fmt_offset + 8)?
                .try_into()
                .ok()?,
        ) as usize;
        let fmt_payload_offset = fmt_offset + 8;
        let fmt_end = fmt_payload_offset.checked_add(fmt_size)?;
        if fmt_size < 16 || fmt_end > prefix.len() {
            pos = fmt_offset + 1;
            continue;
        }
        let Some(format) = parse_wave_format(&prefix[fmt_payload_offset..fmt_end]) else {
            pos = fmt_offset + 1;
            continue;
        };
        let mut chunk_pos = fmt_end + (fmt_size & 1);
        while chunk_pos + 8 <= prefix.len() {
            let tag = prefix.get(chunk_pos..chunk_pos + 4)?;
            let size =
                u32::from_le_bytes(prefix.get(chunk_pos + 4..chunk_pos + 8)?.try_into().ok()?)
                    as usize;
            if tag == b"data" {
                return Some(SharedWaveStream {
                    fmt_offset,
                    data_offset: chunk_pos + 8,
                    data_size: size,
                    format,
                });
            }
            if !is_ascii_chunk_tag(tag) {
                break;
            }
            let next = chunk_pos
                .checked_add(8)?
                .checked_add(size)?
                .checked_add(size & 1)?;
            if next <= chunk_pos || next > prefix.len() {
                break;
            }
            chunk_pos = next;
        }
        pos = fmt_offset + 1;
    }
    None
}

fn parse_wave_format(payload: &[u8]) -> Option<WaveFormat> {
    if payload.len() < 16 {
        return None;
    }
    let format = WaveFormat {
        format_tag: u16::from_le_bytes(payload[0..2].try_into().ok()?),
        channels: u16::from_le_bytes(payload[2..4].try_into().ok()?),
        sample_rate: u32::from_le_bytes(payload[4..8].try_into().ok()?),
        byte_rate: u32::from_le_bytes(payload[8..12].try_into().ok()?),
        block_align: u16::from_le_bytes(payload[12..14].try_into().ok()?),
        bits_per_sample: u16::from_le_bytes(payload[14..16].try_into().ok()?),
    };
    (format.channels > 0
        && format.sample_rate > 0
        && format.byte_rate > 0
        && format.block_align > 0)
        .then_some(format)
}

fn wave_chunks_for_slice(format: WaveFormat, data: &[u8]) -> Vec<u8> {
    let mut fmt_payload = Vec::with_capacity(16);
    fmt_payload.extend_from_slice(&format.format_tag.to_le_bytes());
    fmt_payload.extend_from_slice(&format.channels.to_le_bytes());
    fmt_payload.extend_from_slice(&format.sample_rate.to_le_bytes());
    fmt_payload.extend_from_slice(&format.byte_rate.to_le_bytes());
    fmt_payload.extend_from_slice(&format.block_align.to_le_bytes());
    fmt_payload.extend_from_slice(&format.bits_per_sample.to_le_bytes());

    let mut chunks = Vec::with_capacity(8 + fmt_payload.len() + 8 + data.len());
    chunks.extend_from_slice(b"fmt ");
    chunks.extend_from_slice(&(fmt_payload.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&fmt_payload);
    chunks.extend_from_slice(b"data");
    chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
    chunks.extend_from_slice(data);
    chunks
}

fn make_riff_wave(chunks: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + chunks.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(4_u32 + chunks.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(chunks);
    out
}

fn is_ascii_chunk_tag(tag: &[u8]) -> bool {
    tag.len() == 4 && tag.iter().all(|byte| (32..=126).contains(byte))
}

fn mp3_frame_sync(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0xff && data[1] & 0xe0 == 0xe0
}

fn trailing_zero_count(data: &[u8], start: usize) -> usize {
    let mut count = 0;
    let mut pos = data.len();
    while pos > start && data[pos - 1] == 0 {
        count += 1;
        pos -= 1;
    }
    count
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_wave_chunks_in_riff_header() {
        let chunks = wave_chunks_for_slice(
            WaveFormat {
                format_tag: 1,
                channels: 1,
                sample_rate: 8000,
                byte_rate: 8000,
                block_align: 1,
                bits_per_sample: 8,
            },
            b"\x80\x81",
        );

        let (bytes, result) = pcmdata_portable_audio_bytes(0, &chunks, &[]).unwrap();

        assert!(bytes.starts_with(b"RIFF"));
        assert_eq!(result.media_kind, PcmDataMediaKind::Wave);
    }

    #[test]
    fn extracts_mp3_from_wave_chunks() {
        let chunks = wave_chunks_for_slice(
            WaveFormat {
                format_tag: 0x0055,
                channels: 2,
                sample_rate: 44_100,
                byte_rate: 16_000,
                block_align: 1,
                bits_per_sample: 0,
            },
            b"ID3mp3",
        );

        let (bytes, result) = pcmdata_portable_audio_bytes(0, &chunks, &[]).unwrap();

        assert_eq!(bytes, b"ID3mp3");
        assert_eq!(result.media_kind, PcmDataMediaKind::Mp3);
    }

    #[test]
    fn reconstructs_shared_wave_slice() {
        let format = WaveFormat {
            format_tag: 1,
            channels: 1,
            sample_rate: 8000,
            byte_rate: 8000,
            block_align: 1,
            bits_per_sample: 8,
        };
        let prefix = wave_chunks_for_slice(format, b"abcdef");
        let data_offset = prefix
            .windows(4)
            .position(|window| window == b"data")
            .unwrap()
            + 8;

        let (bytes, result) =
            pcmdata_portable_audio_bytes(data_offset + 2, b"cd", &prefix).unwrap();

        assert!(bytes.starts_with(b"RIFF"));
        assert!(bytes.ends_with(b"cd"));
        assert_eq!(result.media_kind, PcmDataMediaKind::Wave);
    }

    #[test]
    fn truncated_wave_chunks_do_not_panic() {
        let chunks = wave_chunks_for_slice(
            WaveFormat {
                format_tag: 1,
                channels: 1,
                sample_rate: 8000,
                byte_rate: 8000,
                block_align: 1,
                bits_per_sample: 8,
            },
            b"abcdef",
        );

        for len in 0..=chunks.len() {
            let raw = &chunks[..len];
            let _ = pcmdata_audio_summary(0, raw, &[]);
            let _ = pcmdata_portable_audio_bytes(0, raw, &[]);
        }
    }

    #[test]
    fn oversized_riff_chunk_does_not_panic_or_wrap() {
        let mut data = Vec::new();
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        data.extend_from_slice(&[0; 16]);

        assert!(pcmdata_audio_summary(0, &data, &[]).is_err());
        assert!(pcmdata_portable_audio_bytes(0, &data, &[]).is_err());
    }

    #[test]
    fn shared_wave_range_rejects_overflowing_data_extent() {
        let stream = SharedWaveStream {
            fmt_offset: 0,
            data_offset: usize::MAX - 1,
            data_size: 8,
            format: WaveFormat {
                format_tag: 1,
                channels: 1,
                sample_rate: 8000,
                byte_rate: 8000,
                block_align: 1,
                bits_per_sample: 8,
            },
        };

        assert!(!stream.contains(usize::MAX - 1, 1));
    }
}
