use super::*;

impl ReaderBookPackage {
    pub(super) fn open_hourei_law_tree_surface(
        &self,
        surface_id: &str,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.hourei_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei law tree requires an opened Hourei store",
                )],
            });
        };
        let categories = store.categories_with_laws()?;
        let nodes = categories
            .into_iter()
            .map(|category| {
                let children = category
                    .laws
                    .into_iter()
                    .map(|law| {
                        let label = hourei_law_node_label(&law);
                        Ok(NavigationNode {
                            node_id: format!("law:{}", law.hore_id),
                            label_html: escape_hourei_label_html(&label),
                            label_text: label,
                            target: Some(TargetToken::new(&InternalTarget::HoureiLaw {
                                hore_id: law.hore_id,
                                anchor: None,
                            })?),
                            diagnostics: Vec::new(),
                            children: Vec::new(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(NavigationNode {
                    node_id: format!("category:{}", category.id),
                    label_html: escape_hourei_label_html(&category.name),
                    label_text: category.name,
                    target: None,
                    diagnostics: Vec::new(),
                    children,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }
}
