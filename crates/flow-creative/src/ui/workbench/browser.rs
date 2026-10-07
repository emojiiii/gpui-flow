use crate::ProjectSummary;

pub(super) const PAGE_SIZE: usize = 12;

pub(super) struct ProjectPage<'a> {
    pub items: Vec<&'a ProjectSummary>,
    pub matches: usize,
    pub index: usize,
    pub pages: usize,
}

/// Filtering never reorders the saved-project listing or treats names as IDs.
pub(super) fn project_page<'a>(
    projects: &'a [ProjectSummary],
    query: &str,
    requested_page: usize,
) -> ProjectPage<'a> {
    let query = query.trim().to_lowercase();
    let matches: Vec<_> = projects
        .iter()
        .filter(|project| query.is_empty() || project.metadata.name.to_lowercase().contains(&query))
        .collect();
    let count = matches.len();
    let pages = count.div_ceil(PAGE_SIZE).max(1);
    let index = requested_page.min(pages - 1);
    ProjectPage {
        items: matches
            .into_iter()
            .skip(index * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect(),
        matches: count,
        index,
        pages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectMetadata;

    #[test]
    fn filter_before_paging_keeps_identity_order_and_metadata() {
        let projects: Vec<_> = (0..40)
            .map(|i| ProjectSummary {
                metadata: ProjectMetadata {
                    id: format!("project-{i}"),
                    name: if i % 2 == 0 { "封面 CAFÉ" } else { "Other" }.into(),
                    created_at_ms: i,
                    updated_at_ms: 100 - i,
                },
                card_count: 2,
                asset_count: 1,
            })
            .collect();
        let unchanged = projects.clone();
        let page = project_page(&projects, "  café  ", 1);
        assert_eq!((page.matches, page.index, page.pages), (20, 1, 2));
        assert_eq!(page.items.len(), 8);
        assert_eq!(page.items[0].metadata.id, "project-24");
        assert_eq!(page.items[7].metadata.id, "project-38");
        assert_eq!(project_page(&projects, "封面", 0).items.len(), PAGE_SIZE);
        assert_eq!(project_page(&projects, "", 0).matches, 40);
        assert_eq!(project_page(&projects, "project-24", 0).matches, 0);
        assert_eq!(projects, unchanged);
    }

    #[test]
    fn clamped_and_empty_pages_are_bounded() {
        let empty = project_page(&[], "", usize::MAX);
        assert_eq!((empty.matches, empty.index, empty.pages), (0, 0, 1));
        assert!(empty.items.is_empty());
        let project = ProjectSummary {
            metadata: ProjectMetadata {
                id: "one".into(),
                name: "one".into(),
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            card_count: 0,
            asset_count: 0,
        };
        let projects = vec![project; PAGE_SIZE];
        let last = project_page(&projects, "  ", usize::MAX);
        assert_eq!((last.matches, last.index, last.pages), (PAGE_SIZE, 0, 1));
        assert_eq!(last.items.len(), PAGE_SIZE);
        assert!(project_page(&projects, "missing", 8).items.is_empty());
    }
}
