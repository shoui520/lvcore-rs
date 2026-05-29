use crate::error::Result;
use crate::navigation::NavigationNode;
use crate::resources::{InternalResource, ResourceKind, ResourceToken};
use crate::target::{InternalTarget, TargetToken};

use super::html::escape_plain_label_html;
use super::html::{
    PackageHtmlReference, html_attr_value, html_unescape_minimal, normalize_package_relative_path,
    path_has_extension,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChmHhcTocItem {
    pub(super) name: String,
    pub(super) local: Option<String>,
    pub(super) depth: usize,
}

pub(super) fn chm_hanrei_entry_sort_key(path: &str) -> (u8, String) {
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let priority = match file_name.as_str() {
        "top.htm" | "top.html" | "index.htm" | "index.html" => 0,
        "hanrei.htm" | "hanrei.html" => 1,
        "copyright.htm" | "copyright.html" => 9,
        _ => 5,
    };
    (priority, path.to_ascii_lowercase())
}

pub(super) fn parse_chm_hhc_toc(html: &str) -> Vec<ChmHhcTocItem> {
    let lower = html.to_ascii_lowercase();
    let mut items = Vec::new();
    let mut cursor = 0usize;
    let mut depth = 0usize;
    while cursor < lower.len() {
        let next_ul = lower[cursor..].find("<ul").map(|offset| cursor + offset);
        let next_ul_end = lower[cursor..].find("</ul").map(|offset| cursor + offset);
        let next_object = lower[cursor..]
            .find("<object")
            .map(|offset| cursor + offset);
        let Some(next) = [next_ul, next_ul_end, next_object]
            .into_iter()
            .flatten()
            .min()
        else {
            break;
        };
        if Some(next) == next_ul {
            depth += 1;
            cursor = lower[next..]
                .find('>')
                .map(|offset| next + offset + 1)
                .unwrap_or(lower.len());
        } else if Some(next) == next_ul_end {
            depth = depth.saturating_sub(1);
            cursor = lower[next..]
                .find('>')
                .map(|offset| next + offset + 1)
                .unwrap_or(lower.len());
        } else {
            let Some(relative_end) = lower[next..].find("</object>") else {
                break;
            };
            let end = next + relative_end + "</object>".len();
            let block = &html[next..end];
            if block.to_ascii_lowercase().contains("text/sitemap")
                && let Some(name) = chm_hhc_param_value(block, "name")
            {
                items.push(ChmHhcTocItem {
                    name,
                    local: chm_hhc_param_value(block, "local"),
                    depth: depth.saturating_sub(1),
                });
            }
            cursor = end;
        }
    }
    items
}

fn chm_hhc_param_value(block: &str, wanted_name: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find("<param") {
        let start = cursor + relative_start;
        let Some(relative_end) = lower[start..].find('>') else {
            break;
        };
        let end = start + relative_end + 1;
        let tag = &block[start..end];
        let Some(name) = html_attr_value(tag, "name") else {
            cursor = end;
            continue;
        };
        if name.eq_ignore_ascii_case(wanted_name) {
            return html_attr_value(tag, "value");
        }
        cursor = end;
    }
    None
}

pub(super) fn chm_hhc_toc_items_to_nodes(
    chm_path: &str,
    items: &[ChmHhcTocItem],
) -> Result<Vec<NavigationNode>> {
    let mut index = 0usize;
    build_chm_hhc_nodes(chm_path, items, &mut index, 0)
}

fn build_chm_hhc_nodes(
    chm_path: &str,
    items: &[ChmHhcTocItem],
    index: &mut usize,
    depth: usize,
) -> Result<Vec<NavigationNode>> {
    let mut nodes = Vec::new();
    while let Some(item) = items.get(*index) {
        if item.depth < depth {
            break;
        }
        if item.depth > depth {
            break;
        }
        let node_index = *index;
        *index += 1;
        let mut node = chm_hhc_item_to_node(chm_path, item, node_index)?;
        node.children = build_chm_hhc_nodes(chm_path, items, index, depth + 1)?;
        nodes.push(node);
    }
    Ok(nodes)
}

fn chm_hhc_item_to_node(
    chm_path: &str,
    item: &ChmHhcTocItem,
    index: usize,
) -> Result<NavigationNode> {
    let target = item
        .local
        .as_deref()
        .and_then(chm_local_reference)
        .filter(|reference| path_has_extension(&reference.path, &["html", "htm"]))
        .map(|reference| {
            let resource = InternalResource::ChmFile {
                chm_path: chm_path.to_owned(),
                entry_path: reference.path,
                resource_kind: ResourceKind::Html,
            };
            let resource = ResourceToken::new(&resource)?;
            TargetToken::new(&InternalTarget::Resource {
                resource,
                anchor: reference.anchor,
            })
        })
        .transpose()?;
    Ok(NavigationNode {
        node_id: format!("hanrei-chm-toc-{index}"),
        label_html: escape_plain_label_html(&item.name),
        label_text: item.name.clone(),
        target,
        diagnostics: Vec::new(),
        children: Vec::new(),
    })
}

pub(super) fn chm_local_reference(raw_value: &str) -> Option<PackageHtmlReference> {
    let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
    let value = value.trim_start_matches('/');
    let (path_part, anchor) = value.split_once('#').unwrap_or((value, ""));
    let path_part = path_part.split('?').next().unwrap_or("").trim();
    if path_part.is_empty() {
        return None;
    }
    Some(PackageHtmlReference {
        path: normalize_package_relative_path(path_part)?,
        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chm_hhc_toc_labels_and_anchors() {
        let items = parse_chm_hhc_toc(
            r#"
            <UL>
            <OBJECT type="text/sitemap">
              <param name="Name" value="編集方針">
              <param name="Local" value="Source/contents/hanrei_01.htm#midasigo">
            </OBJECT>
            <UL>
            <OBJECT type="text/sitemap">
              <param name="Name" value="見出し語">
              <param name="Local" value="Source/contents/hanrei_01.htm#midasigo_child">
            </OBJECT>
            </UL>
            <OBJECT type="text/sitemap">
              <param name="Name" value="付録">
            </OBJECT>
            <OBJECT type="text/sitemap">
              <param name="Name" value="著作権">
              <param name="Local" value="Source/contents/copyright.htm">
            </OBJECT>
            </UL>
            "#,
        );
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].name, "編集方針");
        assert_eq!(items[0].depth, 0);
        assert_eq!(items[1].name, "見出し語");
        assert_eq!(items[1].depth, 1);
        assert_eq!(items[2].name, "付録");
        assert!(items[2].local.is_none());
        let reference = chm_local_reference(items[0].local.as_deref().unwrap()).unwrap();
        assert_eq!(reference.path, "Source/contents/hanrei_01.htm");
        assert_eq!(reference.anchor.as_deref(), Some("midasigo"));

        let nodes = chm_hhc_toc_items_to_nodes("HANREI.chm", &items).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].label_text, "編集方針");
        assert!(nodes[0].target.is_some());
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].label_text, "見出し語");
        assert!(nodes[1].target.is_none());
    }
}
