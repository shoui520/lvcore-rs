use crate::render::ResolvedTargetKind;
use crate::resources::ResourceKind;

pub(super) const MONOSCR_WIDTH: u32 = 64;
pub(super) const MONOSCR_HEIGHT: u32 = 64;
pub(super) const MONOSCR_BITMAP_BYTES: usize =
    (MONOSCR_WIDTH as usize * MONOSCR_HEIGHT as usize) / 8;

pub(super) fn resource_kind_from_path(path: &str) -> ResourceKind {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mp3") || lower.ends_with(".wav") {
        ResourceKind::Audio
    } else if lower.ends_with(".mp4")
        || lower.ends_with(".m4v")
        || lower.ends_with(".mpg")
        || lower.ends_with(".mpeg")
        || lower.ends_with(".mov")
    {
        ResourceKind::Video
    } else if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
        || lower.ends_with(".bmp")
    {
        ResourceKind::Image
    } else if lower.ends_with(".css") {
        ResourceKind::Css
    } else if lower.ends_with(".js") {
        ResourceKind::Javascript
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        ResourceKind::Html
    } else if lower.ends_with(".pdf") {
        ResourceKind::Pdf
    } else {
        ResourceKind::Other
    }
}

pub(super) fn resource_mime_type(
    kind: ResourceKind,
    path_hint: Option<&str>,
) -> Option<&'static str> {
    let lower = path_hint.map(str::to_ascii_lowercase).unwrap_or_default();
    let from_path = if lower.ends_with(".svg") {
        Some("image/svg+xml")
    } else if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".bmp") {
        Some("image/bmp")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else if lower.ends_with(".mp3") {
        Some("audio/mpeg")
    } else if lower.ends_with(".wav") {
        Some("audio/wav")
    } else if lower.ends_with(".ogg") {
        Some("audio/ogg")
    } else if lower.ends_with(".m4a") {
        Some("audio/mp4")
    } else if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
        Some("video/mp4")
    } else if lower.ends_with(".mpg") || lower.ends_with(".mpeg") {
        Some("video/mpeg")
    } else if lower.ends_with(".mov") {
        Some("video/quicktime")
    } else if lower.ends_with(".css") {
        Some("text/css; charset=utf-8")
    } else if lower.ends_with(".js") {
        Some("text/javascript; charset=utf-8")
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        Some("text/html; charset=utf-8")
    } else if lower.ends_with(".pdf") {
        Some("application/pdf")
    } else if lower.ends_with(".ttf") {
        Some("font/ttf")
    } else if lower.ends_with(".otf") {
        Some("font/otf")
    } else if lower.ends_with(".woff") {
        Some("font/woff")
    } else if lower.ends_with(".woff2") {
        Some("font/woff2")
    } else {
        None
    };
    from_path.or(match kind {
        ResourceKind::Html => Some("text/html; charset=utf-8"),
        ResourceKind::Css => Some("text/css; charset=utf-8"),
        ResourceKind::Javascript => Some("text/javascript; charset=utf-8"),
        ResourceKind::Pdf => Some("application/pdf"),
        ResourceKind::Image => Some("image/png"),
        ResourceKind::Colscr => Some("image/bmp"),
        ResourceKind::PcmData => Some("audio/wav"),
        ResourceKind::SoundData => Some("audio/wav"),
        ResourceKind::Video => Some("video/mpeg"),
        _ => None,
    })
}

pub(super) fn parse_colscr_wrapped_payload_size(data: &[u8]) -> Option<usize> {
    if data.len() < 12 || &data[..4] != b"data" {
        return None;
    }
    let payload_size = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    if payload_size == 0 {
        return None;
    }
    let image = &data[8..];
    if image.starts_with(b"BM")
        || image.starts_with(b"\xff\xd8\xff")
        || image.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Some(payload_size);
    }
    None
}

pub(super) fn monoscr_bitmap_to_rgba(bitmap: &[u8]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(MONOSCR_WIDTH as usize * MONOSCR_HEIGHT as usize * 4);
    for byte in bitmap {
        for bit in 0..8 {
            if byte & (0x80 >> bit) != 0 {
                pixels.extend_from_slice(&[0, 0, 0, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    pixels
}

pub(super) fn resolved_kind_for_package_html_path(path: &str) -> ResolvedTargetKind {
    let lower = path.to_ascii_lowercase();
    if lower.contains("hanrei")
        || lower.contains("_help.localized/")
        || lower.starts_with("hanrei/")
        || lower.starts_with("hanrei.")
    {
        ResolvedTargetKind::HanreiPage
    } else {
        ResolvedTargetKind::InfoPage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_resource_kind_and_mime_from_path() {
        assert_eq!(
            resource_kind_from_path("Templates/B123.svg"),
            ResourceKind::Image
        );
        assert_eq!(resource_kind_from_path("movie.MPG"), ResourceKind::Video);
        assert_eq!(
            resource_mime_type(ResourceKind::Other, Some("style.css")),
            Some("text/css; charset=utf-8")
        );
        assert_eq!(
            resource_mime_type(ResourceKind::PcmData, Some("PCMDATA.DIC")),
            Some("audio/wav")
        );
    }

    #[test]
    fn detects_colscr_wrapped_bitmap_payload() {
        let mut wrapped = b"data".to_vec();
        wrapped.extend_from_slice(&4_u32.to_le_bytes());
        wrapped.extend_from_slice(b"BM\0\0");
        assert_eq!(parse_colscr_wrapped_payload_size(&wrapped), Some(4));
    }
}
