use super::*;

pub(super) struct SsedMultiSurfaceId {
    pub(super) descriptor: String,
    pub(super) record_index: Option<u16>,
    pub(super) filter: Option<String>,
}

pub(super) fn parse_ssed_multi_surface_id(surface_id: &str) -> Option<SsedMultiSurfaceId> {
    let rest = surface_id.strip_prefix("multi:")?;
    let parts = rest.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [descriptor] if !descriptor.is_empty() => Some(SsedMultiSurfaceId {
            descriptor: (*descriptor).to_owned(),
            record_index: None,
            filter: None,
        }),
        [descriptor, "record", record_index] if !descriptor.is_empty() => {
            Some(SsedMultiSurfaceId {
                descriptor: (*descriptor).to_owned(),
                record_index: record_index.parse().ok(),
                filter: None,
            })
            .filter(|parsed| parsed.record_index.is_some())
        }
        [descriptor, "record", record_index, "filter", filter_hex] if !descriptor.is_empty() => {
            let filter = hex::decode(filter_hex)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())?;
            Some(SsedMultiSurfaceId {
                descriptor: (*descriptor).to_owned(),
                record_index: Some(record_index.parse().ok()?),
                filter: Some(filter),
            })
        }
        _ => None,
    }
}

pub(super) fn ssed_multi_root_surface_id(descriptor_name: &str) -> String {
    format!("multi:{descriptor_name}")
}

pub(super) fn ssed_multi_record_surface_id(
    descriptor_name: &str,
    record_index: u16,
    filter: Option<&str>,
) -> String {
    match filter {
        Some(filter) => format!(
            "multi:{descriptor_name}:record:{record_index}:filter:{}",
            hex::encode(filter.as_bytes())
        ),
        None => format!("multi:{descriptor_name}:record:{record_index}"),
    }
}

pub(super) fn ssed_multi_record_menu_ref(
    record: &SsedMultiRecord,
) -> Option<&SsedMultiComponentRef> {
    record
        .refs
        .iter()
        .find(|reference| reference.component_type == 0x01)
}

pub(super) fn ssed_multi_record_index_ref(
    record: &SsedMultiRecord,
) -> Option<&SsedMultiComponentRef> {
    record
        .refs
        .iter()
        .find(|reference| is_supported_index_type(reference.component_type))
}
