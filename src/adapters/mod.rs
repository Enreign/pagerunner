pub struct SeedAdapter {
    pub origin: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub js_code: &'static str,
}

pub fn seed_adapters() -> &'static [SeedAdapter] {
    &[
        SeedAdapter {
            origin: "https://github.com",
            name: "list-issues",
            description:
                "List issues for a GitHub repository. params: { owner, repo, state?, limit? }",
            js_code: include_str!("github_list_issues.js"),
        },
        SeedAdapter {
            origin: "https://github.com",
            name: "create-issue",
            description: "Create a GitHub issue. params: { owner, repo, title, body?, labels? }",
            js_code: include_str!("github_create_issue.js"),
        },
        SeedAdapter {
            origin: "https://github.com",
            name: "search-issues",
            description:
                "Search GitHub issues using GitHub search syntax. params: { query, limit? }",
            js_code: include_str!("github_search_issues.js"),
        },
        SeedAdapter {
            origin: "https://linear.app",
            name: "create-issue",
            description:
                "Create a Linear issue. params: { team_id, title, description?, state_id? }",
            js_code: include_str!("linear_create_issue.js"),
        },
        SeedAdapter {
            origin: "https://linear.app",
            name: "create-comment",
            description: "Add a comment to a Linear issue. params: { issue_id, body }",
            js_code: include_str!("linear_create_comment.js"),
        },
        SeedAdapter {
            origin: "https://linear.app",
            name: "update-status",
            description: "Update the status of a Linear issue. params: { issue_id, state_id }",
            js_code: include_str!("linear_update_status.js"),
        },
        SeedAdapter {
            origin: "https://jira.atlassian.com",
            name: "create-issue",
            description:
                "Create a Jira issue. params: { project_key, summary, issue_type?, description? }",
            js_code: include_str!("jira_create_issue.js"),
        },
        SeedAdapter {
            origin: "https://jira.atlassian.com",
            name: "transition-issue",
            description:
                "Transition a Jira issue to a new status. params: { issue_key, transition_id }",
            js_code: include_str!("jira_transition_issue.js"),
        },
        SeedAdapter {
            origin: "https://www.notion.so",
            name: "create-page",
            description: "Create a Notion page. params: { parent_id, title, content? }",
            js_code: include_str!("notion_create_page.js"),
        },
        SeedAdapter {
            origin: "https://www.notion.so",
            name: "append-block",
            description: "Append a paragraph block to a Notion page. params: { page_id, content }",
            js_code: include_str!("notion_append_block.js"),
        },
        SeedAdapter {
            origin: "https://mail.google.com",
            name: "list-messages",
            description: "List Gmail messages. params: { query?, max_results? }",
            js_code: include_str!("gmail_list_messages.js"),
        },
        SeedAdapter {
            origin: "https://mail.google.com",
            name: "get-message",
            description: "Get a Gmail message by ID. params: { message_id, format? }",
            js_code: include_str!("gmail_get_message.js"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_adapters_not_empty() {
        let adapters = seed_adapters();
        assert!(!adapters.is_empty());
    }

    #[test]
    fn all_seed_adapters_have_non_empty_fields() {
        for adapter in seed_adapters() {
            assert!(
                !adapter.origin.is_empty(),
                "origin empty for {}",
                adapter.name
            );
            assert!(!adapter.name.is_empty());
            assert!(
                !adapter.description.is_empty(),
                "description empty for {}",
                adapter.name
            );
            assert!(
                !adapter.js_code.is_empty(),
                "js_code empty for {}",
                adapter.name
            );
        }
    }

    #[test]
    fn seed_adapters_have_valid_origins() {
        for adapter in seed_adapters() {
            assert!(
                adapter.origin.starts_with("https://"),
                "origin '{}' must start with https://",
                adapter.origin
            );
        }
    }

    #[test]
    fn no_duplicate_origin_name_pairs() {
        let adapters = seed_adapters();
        let mut seen = std::collections::HashSet::new();
        for a in adapters {
            let key = format!("{}::{}", a.origin, a.name);
            assert!(seen.insert(key.clone()), "duplicate: {}", key);
        }
    }
}
