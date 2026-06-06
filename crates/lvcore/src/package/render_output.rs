use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::json;

use super::html::{HtmlAttrName, html_basic_text, next_html_href_or_src_attr};
use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::render::{RenderCapability, RenderMode, RenderOptions, ResolvedTargetView};
use crate::resources::ResourceKind;

const GENERIC_HTML_INLINE_RESOURCE_MAX_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn generic_html_inline_resource_max_bytes() -> usize {
    GENERIC_HTML_INLINE_RESOURCE_MAX_BYTES
}

pub(super) fn generic_html_data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        mime_type,
        BASE64_STANDARD.encode(bytes)
    )
}

pub(super) fn finalize_generic_html_view<F>(
    mut view: ResolvedTargetView,
    mut data_url_for_resource: F,
) -> Result<ResolvedTargetView>
where
    F: FnMut(&str) -> Result<Option<String>>,
{
    let Some(html) = view.display_html.take() else {
        return Ok(view);
    };
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0usize;
    let lower = html.to_ascii_lowercase();
    let mut inlined_resources = 0usize;
    let mut target_links = 0usize;

    while let Some(attr) = next_html_href_or_src_attr(&html, &lower, cursor) {
        output.push_str(&html[cursor..attr.value_start]);
        let raw_value = &html[attr.value_start..attr.value_end];
        if let Some(token) = raw_value.strip_prefix("lvcore://resource/") {
            let token = token
                .split_once(['?', '#'])
                .map(|(token, _)| token)
                .unwrap_or(token);
            match data_url_for_resource(token) {
                Ok(Some(data_url)) => {
                    output.push_str(&data_url);
                    inlined_resources += 1;
                }
                Ok(None) => output.push_str(raw_value),
                Err(error) => {
                    output.push_str("data:,");
                    view.diagnostics.push(Diagnostic::warning(
                        "generic_html_resource_inline_failed",
                        error.to_string(),
                    ));
                }
            }
        } else if attr.name == HtmlAttrName::Href
            && let Some(token) = raw_value.strip_prefix("lvcore://target/")
        {
            output.push_str("#lvcore-target-");
            output.push_str(token);
            target_links += 1;
        } else {
            output.push_str(raw_value);
        }
        cursor = attr.value_end;
    }
    output.push_str(&html[cursor..]);

    if inlined_resources > 0 {
        view.diagnostics.push(Diagnostic::info(
            "generic_html_resources_inlined",
            format!("{inlined_resources} lvcore resources were embedded as data URLs"),
        ));
    }
    if target_links > 0 {
        view.diagnostics.push(Diagnostic::info(
            "generic_html_targets_fragmentized",
            format!("{target_links} lvcore target links were converted to local fragments"),
        ));
    }
    if output.contains("lvcore://target/") || output.contains("lvcore://resource/") {
        view.diagnostics.push(Diagnostic::warning(
            "generic_html_router_reference_remaining",
            "GenericHtml output still contains lvcore router references that could not be rewritten",
        ));
    }
    view.display_html = Some(output);
    Ok(view)
}

pub(super) fn finalize_resolved_view(
    mut view: ResolvedTargetView,
    options: &RenderOptions,
) -> ResolvedTargetView {
    if view.href.is_empty() {
        view.href = view.target.href();
    }
    for link in &mut view.links {
        if link.href.is_empty() {
            link.href = link.token.href();
        }
    }
    update_visual_capabilities(&mut view);

    match options.mode {
        RenderMode::Native => {}
        RenderMode::BasicText => {
            if let Some(html) = view.display_html.take() {
                view.basic_text = Some(html_basic_text(&html));
                view.resources.clear();
                view.links.clear();
                view.capabilities.clear();
            }
        }
        RenderMode::GenericHtml => {
            if view.display_html.as_deref().is_some_and(|html| {
                html.contains("lvcore://target/") || html.contains("lvcore://resource/")
            }) {
                view.diagnostics.push(Diagnostic::info(
                    "generic_html_router_required",
                    "GenericHtml output still contains lvcore:// links or resources that could not be converted to standalone browser references",
                ));
            }
        }
        RenderMode::Debug => {}
    }

    if (options.include_debug_trace || options.mode == RenderMode::Debug)
        && view.debug_trace.is_none()
    {
        view.debug_trace = Some(
            json!({
                "mode": options.mode,
                "kind": view.kind,
                "target": view.target.clone(),
                "title": view.title.clone(),
                "has_display_html": view.display_html.is_some(),
                "has_basic_text": view.basic_text.is_some(),
                "resource_count": view.resources.len(),
                "link_count": view.links.len(),
                "capabilities": view.capabilities.clone(),
                "diagnostics": view.diagnostics.clone(),
            })
            .to_string(),
        );
    }

    view
}

fn update_visual_capabilities(view: &mut ResolvedTargetView) {
    if let Some(html) = view.display_html.as_deref() {
        push_render_capability_once(&mut view.capabilities, RenderCapability::Html);

        let lower = html.to_ascii_lowercase();
        if lower.contains("<script") || lower.contains(".js") {
            push_render_capability_once(&mut view.capabilities, RenderCapability::Javascript);
        }
        if lower.contains("<style") || lower.contains("stylesheet") || lower.contains(".css") {
            push_render_capability_once(&mut view.capabilities, RenderCapability::Css);
        }
        if lower.contains("mathjax")
            || lower.contains("tex-mml")
            || lower.contains("<math")
            || html.contains(r"\(")
            || html.contains(r"\[")
            || html.contains("$$")
        {
            push_render_capability_once(&mut view.capabilities, RenderCapability::MathJax);
        }
        if lower.contains("writing-mode")
            || lower.contains("vertical-rl")
            || lower.contains("tb-rl")
            || lower.contains("tategaki")
        {
            push_render_capability_once(&mut view.capabilities, RenderCapability::VerticalText);
        }
    }

    for resource in &view.resources {
        match resource.kind {
            ResourceKind::Image | ResourceKind::Template | ResourceKind::Colscr => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Images);
            }
            ResourceKind::Audio | ResourceKind::PcmData | ResourceKind::SoundData => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Audio);
            }
            ResourceKind::Video => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Video);
            }
            ResourceKind::Css => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Css);
            }
            ResourceKind::Javascript => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Javascript);
            }
            _ => {}
        }
    }
}

fn push_render_capability_once(
    capabilities: &mut Vec<RenderCapability>,
    capability: RenderCapability,
) {
    if !capabilities.contains(&capability) {
        capabilities.push(capability);
    }
}

#[cfg(test)]
mod tests {
    use crate::render::{RenderMode, RenderOptions, ResolvedTargetKind, ResolvedTargetView};
    use crate::target::{InternalTarget, TargetKind, TargetLink, TargetToken};

    use super::{finalize_generic_html_view, finalize_resolved_view};

    fn token(label: &str) -> TargetToken {
        TargetToken::new(&InternalTarget::Unsupported {
            reason: label.to_owned(),
        })
        .unwrap()
    }

    #[test]
    fn generic_html_finalizer_inlines_resources_and_fragmentizes_targets() {
        let view = ResolvedTargetView {
            href: String::new(),
            kind: ResolvedTargetKind::EntryBody,
            target: token("entry"),
            title: None,
            display_html: Some(
                r#"<a href = "lvcore://target/target-token">next</a><img src = "lvcore://resource/res-token?variant=small#fig">"#
                    .to_owned(),
            ),
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Default::default(),
            diagnostics: Vec::new(),
            debug_trace: None,
        };

        let view = finalize_generic_html_view(view, |token| {
            assert_eq!(token, "res-token");
            Ok(Some("data:image/png;base64,AA==".to_owned()))
        })
        .unwrap();
        let html = view.display_html.as_deref().unwrap();

        assert!(html.contains("#lvcore-target-target-token"));
        assert!(html.contains("data:image/png;base64,AA=="));
        assert!(
            view.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "generic_html_resources_inlined")
        );
        assert!(
            view.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "generic_html_targets_fragmentized")
        );
    }

    #[test]
    fn resolved_view_finalizer_populates_public_target_and_link_hrefs() {
        let target = token("entry");
        let link_target = token("link");
        let view = ResolvedTargetView {
            href: String::new(),
            kind: ResolvedTargetKind::EntryBody,
            target: target.clone(),
            title: None,
            display_html: Some("<p>entry</p>".to_owned()),
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: Vec::new(),
            links: vec![TargetLink {
                href: String::new(),
                token: link_target.clone(),
                label: "link".to_owned(),
                kind: TargetKind::Unsupported,
                diagnostics: Vec::new(),
                attributes: Default::default(),
            }],
            capabilities: Default::default(),
            diagnostics: Vec::new(),
            debug_trace: None,
        };

        let view = finalize_resolved_view(view, &RenderOptions::default());

        assert_eq!(view.href, target.href());
        assert_eq!(view.links[0].href, link_target.href());
    }

    #[test]
    fn generic_html_finalizer_replaces_unreadable_resources_with_empty_data_url() {
        let view = ResolvedTargetView {
            href: String::new(),
            kind: ResolvedTargetKind::InfoPage,
            target: token("entry"),
            title: None,
            display_html: Some(r#"<script src="lvcore://resource/missing"></script>"#.to_owned()),
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Default::default(),
            diagnostics: Vec::new(),
            debug_trace: None,
        };

        let view = finalize_generic_html_view(view, |_| {
            Err(crate::error::Error::Driver("missing".to_owned()))
        })
        .unwrap();
        let html = view.display_html.as_deref().unwrap();

        assert!(html.contains(r#"src="data:,""#));
        assert!(!html.contains("lvcore://resource/"));
        assert!(
            view.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "generic_html_resource_inline_failed")
        );
    }

    #[test]
    fn basic_text_finalizer_flattens_html_and_clears_visual_side_channels() {
        let view = ResolvedTargetView {
            href: String::new(),
            kind: ResolvedTargetKind::EntryBody,
            target: token("entry"),
            title: None,
            display_html: Some("<p>body<br>line</p>".to_owned()),
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Default::default(),
            diagnostics: Vec::new(),
            debug_trace: None,
        };

        let view = finalize_resolved_view(
            view,
            &RenderOptions {
                mode: RenderMode::BasicText,
                ..RenderOptions::default()
            },
        );

        assert_eq!(view.display_html, None);
        assert_eq!(view.basic_text.as_deref(), Some("body\nline"));
    }
}
