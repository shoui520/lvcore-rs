use super::html::html_unescape_minimal;
use crate::resources::{InternalResource, ResourceKind};
use crate::ssed_loose_media::parse_lved_address;
use crate::target::InternalTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LvedHtmlRefKind {
    Media,
    ZipToMedia,
    Image,
    Pdf,
    Address,
    DataId,
    CrossBook,
    Info,
    Binran,
    ViewerHook,
}

pub(super) fn next_lved_ref(value: &str) -> Option<(usize, LvedHtmlRefKind)> {
    let patterns = [
        ("lved.media.", LvedHtmlRefKind::Media),
        ("lved.media:", LvedHtmlRefKind::Media),
        ("lved.sound:", LvedHtmlRefKind::Media),
        ("lved.ziptomedia:", LvedHtmlRefKind::ZipToMedia),
        ("lved.image:", LvedHtmlRefKind::Image),
        ("lved.imag:", LvedHtmlRefKind::Image),
        ("lved.pdf:", LvedHtmlRefKind::Pdf),
        ("lved.addr", LvedHtmlRefKind::Address),
        ("lved.dataid.dict.", LvedHtmlRefKind::CrossBook),
        ("lved.contentlink:", LvedHtmlRefKind::CrossBook),
        ("lved.dataid.result:", LvedHtmlRefKind::DataId),
        ("lved.dataid:", LvedHtmlRefKind::DataId),
        ("lved.dataid", LvedHtmlRefKind::DataId),
        ("lved.info:", LvedHtmlRefKind::Info),
        ("lved.binran:", LvedHtmlRefKind::Binran),
        ("lved.bookmark:", LvedHtmlRefKind::ViewerHook),
        ("lved.plugin:", LvedHtmlRefKind::ViewerHook),
        ("lved.sql:", LvedHtmlRefKind::ViewerHook),
        ("lved.findnum:", LvedHtmlRefKind::ViewerHook),
        ("lved.select:", LvedHtmlRefKind::ViewerHook),
        ("lved.group.", LvedHtmlRefKind::ViewerHook),
        ("lved.browser.", LvedHtmlRefKind::ViewerHook),
    ];
    let mut cursor = 0usize;
    while let Some(relative_index) = value[cursor..].find("lved") {
        let index = cursor + relative_index;
        if !is_lved_ref_boundary(value, index) {
            cursor = index.saturating_add("lved".len());
            continue;
        }
        let rest = &value[index..];
        if let Some((_, kind)) = patterns
            .iter()
            .find(|(pattern, _)| rest.starts_with(pattern))
        {
            return Some((index, *kind));
        }
        cursor = index.saturating_add("lved".len());
    }
    None
}

fn is_lved_ref_boundary(value: &str, index: usize) -> bool {
    index == 0
        || !value
            .as_bytes()
            .get(index - 1)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

pub(super) fn lved_media_resource(raw_ref: &str) -> Option<InternalResource> {
    let (namespace, key) = if let Some(value) = raw_ref.strip_prefix("lved.media.") {
        value.split_once(':')?
    } else if let Some(key) = raw_ref.strip_prefix("lved.media:") {
        ("media", key)
    } else if let Some(key) = raw_ref.strip_prefix("lved.sound:") {
        ("sound", key)
    } else {
        return None;
    };
    let key = lved_resource_key(key)?;
    if key.is_empty() {
        return None;
    }
    let lower_namespace = namespace.to_lowercase();
    let lower_key = key.to_lowercase();
    let audio = lower_namespace.contains("sound")
        || lower_namespace.contains("audio")
        || lower_key.ends_with(".mp3")
        || lower_key.ends_with(".wav");
    let image = lower_namespace.contains("image")
        || lower_namespace.contains("picture")
        || lower_key.ends_with(".png")
        || lower_key.ends_with(".jpg")
        || lower_key.ends_with(".jpeg")
        || lower_key.ends_with(".gif")
        || lower_key.ends_with(".svg")
        || lower_key.ends_with(".bmp");
    let video = lower_namespace.contains("video")
        || lower_namespace.contains("movie")
        || lower_key.ends_with(".mp4")
        || lower_key.ends_with(".m4v")
        || lower_key.ends_with(".mpg")
        || lower_key.ends_with(".mpeg")
        || lower_key.ends_with(".mov");
    let resource_kind = if audio {
        ResourceKind::Audio
    } else if video {
        ResourceKind::Video
    } else if image {
        ResourceKind::Image
    } else {
        ResourceKind::MediaBlob
    };
    let store = if audio { "lved.mediasub" } else { "lved.media" };
    Some(InternalResource::MediaBlob {
        store: store.to_owned(),
        key,
        resource_kind,
    })
}

pub(super) fn lved_ziptomedia_resource(raw_ref: &str) -> Option<InternalResource> {
    let reference = raw_ref
        .strip_prefix("lved.ziptomedia:")
        .and_then(lved_resource_key)?;
    Some(InternalResource::ZipToMedia { reference })
}

pub(super) fn lved_image_resource(raw_ref: &str) -> Option<InternalResource> {
    let key = raw_ref
        .strip_prefix("lved.image:")
        .or_else(|| raw_ref.strip_prefix("lved.imag:"))
        .and_then(lved_resource_key)?;
    Some(InternalResource::MediaBlob {
        store: "lved.media".to_owned(),
        key,
        resource_kind: ResourceKind::Image,
    })
}

pub(super) fn lved_pdf_resource(raw_ref: &str) -> Option<InternalResource> {
    let key = raw_ref
        .strip_prefix("lved.pdf:")
        .and_then(lved_resource_key)?;
    Some(InternalResource::MediaBlob {
        store: "lved.media".to_owned(),
        key,
        resource_kind: ResourceKind::Pdf,
    })
}

pub(super) fn lved_dataid_target(raw_ref: &str) -> Option<InternalTarget> {
    let (content_id, anchor) = lved_dataid_anchor(raw_ref)?;
    let row_id = content_id.parse::<i64>().ok()?;
    Some(InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id,
        anchor,
        query: None,
    })
}

pub(super) fn lved_dataid_anchor(raw_ref: &str) -> Option<(String, Option<String>)> {
    let value = raw_ref
        .strip_prefix("lved.dataid.result:")
        .or_else(|| raw_ref.strip_prefix("lved.dataid:"))
        .or_else(|| raw_ref.strip_prefix("lved.dataid"))?;
    let value = value.strip_prefix(':').unwrap_or(value);
    if value.is_empty() || !value.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let (content_id, anchor) = split_lved_target_anchor(value);
    if content_id.is_empty() || !content_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((
        content_id.to_owned(),
        (!anchor.is_empty()).then(|| anchor.to_owned()),
    ))
}

pub(super) fn lved_cross_book_target(raw_ref: &str) -> Option<InternalTarget> {
    if let Some(value) = raw_ref.strip_prefix("lved.dataid.dict.") {
        let (dict_code, target) = value.split_once(':')?;
        let (content_id, anchor) = split_lved_target_anchor(target);
        if dict_code.is_empty() || content_id.is_empty() {
            return None;
        }
        return Some(InternalTarget::LvedCrossBook {
            link_kind: "dataid-dict".to_owned(),
            dict_code: dict_code.to_owned(),
            content_id: content_id.to_owned(),
            anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
        });
    }
    if let Some(value) = raw_ref.strip_prefix("lved.contentlink:") {
        let (dict_code, target) = value.split_once('.')?;
        let (content_id, anchor) = split_lved_target_anchor(target);
        if dict_code.is_empty() || content_id.is_empty() {
            return None;
        }
        return Some(InternalTarget::LvedCrossBook {
            link_kind: "contentlink".to_owned(),
            dict_code: dict_code.to_owned(),
            content_id: content_id.to_owned(),
            anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
        });
    }
    None
}

pub(super) fn lved_address_target(raw_ref: &str) -> Option<InternalTarget> {
    let address = parse_lved_address(raw_ref)?;
    let suffix = raw_ref
        .find(&address.raw)
        .map(|index| &raw_ref[index + address.raw.len()..])
        .unwrap_or("");
    let anchor = suffix
        .strip_prefix('#')
        .map(html_unescape_minimal)
        .filter(|value| !value.is_empty());
    Some(InternalTarget::LvedAddress {
        block: address.block,
        offset: address.offset,
        raw: html_unescape_minimal(raw_ref),
        anchor,
    })
}

pub(super) fn lved_info_target(raw_ref: &str) -> Option<InternalTarget> {
    let value = raw_ref.strip_prefix("lved.info:")?;
    let (name, anchor) = split_lved_target_anchor(value);
    if name.is_empty() {
        return None;
    }
    Some(InternalTarget::LvedInfoPage {
        name: html_unescape_minimal(name),
        anchor: (!anchor.is_empty()).then(|| html_unescape_minimal(anchor)),
    })
}

pub(super) fn lved_binran_target(raw_ref: &str) -> Option<InternalTarget> {
    let value = raw_ref.strip_prefix("lved.binran:")?;
    let (name, anchor) = split_lved_target_anchor(value);
    if name.is_empty() {
        return None;
    }
    Some(InternalTarget::LvedNamedPage {
        table: "binran".to_owned(),
        name: html_unescape_minimal(name),
        anchor: (!anchor.is_empty()).then(|| html_unescape_minimal(anchor)),
    })
}

pub(super) fn lved_viewer_hook_target(raw_ref: &str) -> InternalTarget {
    let hook = raw_ref
        .strip_prefix("lved.")
        .and_then(|rest| {
            rest.split([':', '.'])
                .next()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("unknown");
    InternalTarget::LvedViewerHook {
        hook: hook.to_owned(),
        value: html_unescape_minimal(raw_ref),
    }
}

pub(super) fn is_lved_ref_terminator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | ')' | ']')
}

fn lved_resource_key(value: &str) -> Option<String> {
    let value = value
        .split_once('?')
        .map_or(value, |(head, _)| head)
        .split_once('#')
        .map_or(value, |(head, _)| head)
        .trim();
    (!value.is_empty()).then(|| html_unescape_minimal(value))
}

fn split_lved_target_anchor(value: &str) -> (&str, &str) {
    let value = value.split_once('?').map_or(value, |(head, _)| head);
    value.split_once('#').unwrap_or((value, ""))
}

#[cfg(test)]
mod tests {
    use crate::resources::ResourceKind;
    use crate::target::InternalTarget;

    use super::*;

    #[test]
    fn classifies_lved_media_resource_references() {
        let Some(InternalResource::MediaBlob {
            store,
            key,
            resource_kind,
        }) = lved_media_resource("lved.media:sound/example.mp3")
        else {
            panic!("expected media blob");
        };

        assert_eq!(store, "lved.mediasub");
        assert_eq!(key, "sound/example.mp3");
        assert_eq!(resource_kind, ResourceKind::Audio);
    }

    #[test]
    fn classifies_lved_ziptomedia_resource_references() {
        assert_eq!(
            next_lved_ref(r#"<a href="lved.ziptomedia:000010.wav">"#),
            Some((9, LvedHtmlRefKind::ZipToMedia))
        );
        let Some(InternalResource::ZipToMedia { reference }) =
            lved_ziptomedia_resource("lved.ziptomedia:000010.wav")
        else {
            panic!("expected ziptomedia resource");
        };

        assert_eq!(reference, "000010.wav");
    }

    #[test]
    fn parses_lved_targets_and_preserves_viewer_hooks() {
        assert!(next_lved_ref("uplved.addr000291540042").is_none());

        assert_eq!(
            lved_dataid_anchor("lved.dataid:00157445#body"),
            Some(("00157445".to_owned(), Some("body".to_owned())))
        );

        let Some(InternalTarget::LvedRow { row_id, anchor, .. }) =
            lved_dataid_target("lved.dataid:123#body")
        else {
            panic!("expected LVED row");
        };
        assert_eq!(row_id, 123);
        assert_eq!(anchor.as_deref(), Some("body"));

        let Some(InternalTarget::LvedCrossBook {
            dict_code,
            content_id,
            ..
        }) = lved_cross_book_target("lved.contentlink:BUREI.400")
        else {
            panic!("expected cross-book target");
        };
        assert_eq!(dict_code, "BUREI");
        assert_eq!(content_id, "400");

        let Some(InternalTarget::LvedAddress {
            block,
            offset,
            anchor,
            ..
        }) = lved_address_target("lved.addr=00029154:0042#jump")
        else {
            panic!("expected LVED address target");
        };
        assert_eq!(block, 0x0002_9154);
        assert_eq!(offset, 0x0042);
        assert_eq!(anchor.as_deref(), Some("jump"));

        let InternalTarget::LvedViewerHook { hook, value } =
            lved_viewer_hook_target("lved.plugin:sample")
        else {
            panic!("expected viewer hook target");
        };
        assert_eq!(hook, "plugin");
        assert_eq!(value, "lved.plugin:sample");
    }
}
