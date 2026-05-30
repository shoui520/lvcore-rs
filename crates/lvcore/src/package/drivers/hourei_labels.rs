pub(super) fn hourei_law_node_label(entry: &crate::hourei::HoureiLawEntry) -> String {
    if let Some(name_sub) = &entry.name_sub
        && !name_sub.trim().is_empty()
    {
        return format!("{} {}", entry.name, name_sub);
    }
    if !entry.name.trim().is_empty() {
        return entry.name.clone();
    }
    if let Some(abbr1) = &entry.abbr1
        && !abbr1.trim().is_empty()
    {
        return abbr1.clone();
    }
    entry.hore_id.clone()
}
