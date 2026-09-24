use super::*;

pub(super) fn tool_run_end(messages: &[ChatEntry], start: usize) -> usize {
    let mut end = start;
    while end < messages.len() && messages[end].role == Role::Tool {
        end += 1;
    }
    end
}

pub(super) fn tool_title_and_status(text: &str) -> (&str, Option<&str>) {
    let headline = text.lines().next().unwrap_or(text);
    let Some((title, status)) = headline.rsplit_once(" · ") else {
        return (headline, None);
    };
    if matches!(status, "pending" | "in_progress" | "completed" | "failed") {
        (title, Some(status))
    } else {
        (headline, None)
    }
}

pub(super) fn markdown_code_block(text: &str) -> String {
    let fence_size = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .max(2)
        + 1;
    let fence = "`".repeat(fence_size);
    format!("{fence}\n{text}\n{fence}")
}

pub(super) fn is_approval_review(text: &str) -> bool {
    tool_title_and_status(text).0 == "Guardian Review"
}

pub(super) fn is_generic_tool_title(text: &str) -> bool {
    matches!(tool_title_and_status(text).0, "Terminal" | "Using tool")
}

pub(super) fn shell_steps(script: &str) -> Vec<String> {
    let mut steps = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut chars = script.chars().peekable();
    while let Some(character) = chars.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            current.push(character);
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            current.push(character);
            if character == open {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            current.push(character);
            quote = Some(character);
            continue;
        }
        if matches!(character, ';' | '|') || (character == '&' && chars.peek() == Some(&'&')) {
            if !current.trim().is_empty() {
                steps.push(current.trim().to_owned());
            }
            current.clear();
            if character == '&' || (character == '|' && chars.peek() == Some(&'|')) {
                chars.next();
            }
            continue;
        }
        current.push(character);
    }
    if !current.trim().is_empty() {
        steps.push(current.trim().to_owned());
    }
    steps
}

pub(super) fn simple_tool_description(script: &str) -> (String, bool) {
    let words = shell_words::split(script).unwrap_or_default();
    let command = words.first().map(String::as_str).unwrap_or_default();
    let rest = words.get(1..).unwrap_or(&[]);
    if words
        .iter()
        .any(|word| matches!(word.as_str(), ">" | ">>" | "<"))
    {
        return (script.to_owned(), false);
    }
    let basename = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    match basename {
        "cd" => ("Change folder".into(), true),
        "rg" if rest.iter().any(|arg| arg == "--files") => ("List files".into(), true),
        "rg" | "grep" => {
            let operands = rest
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if let Some(pattern) = operands.first() {
                let location = if operands.len() > 1 {
                    format!(" in {}", operands[1..].join(", "))
                } else {
                    String::new()
                };
                (format!("Search {pattern}{location}"), true)
            } else {
                (script.to_owned(), false)
            }
        }
        "cat" | "head" | "tail" => {
            let files = rest
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if files.is_empty() {
                (script.to_owned(), false)
            } else {
                (format!("Read {}", files.join(", ")), true)
            }
        }
        "sed"
            if !rest
                .iter()
                .any(|arg| arg == "-i" || arg.starts_with("--in-place")) =>
        {
            if let Some(file) = rest.last().filter(|arg| !arg.starts_with('-')) {
                (format!("Read {file}"), true)
            } else {
                (script.to_owned(), false)
            }
        }
        "ls" => {
            let locations = rest
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if locations.is_empty() {
                ("List files".into(), true)
            } else {
                (format!("List {}", locations.join(", ")), true)
            }
        }
        "pwd" => ("Show working folder".into(), true),
        "find" => (
            format!(
                "Find files in {}",
                rest.first().map(String::as_str).unwrap_or(".")
            ),
            true,
        ),
        "git" => match rest.first().map(String::as_str) {
            Some("status") => ("Check git status".into(), true),
            Some("log") => ("Inspect git history".into(), true),
            Some("diff") => ("Review changes".into(), true),
            Some("show") => ("Inspect commit".into(), true),
            _ => (script.to_owned(), false),
        },
        "wc" => ("Count output".into(), true),
        "xargs" if rest.first().is_some_and(|arg| arg == "wc") => ("Count output".into(), true),
        _ => (script.to_owned(), false),
    }
}

pub(super) fn tool_description(text: &str) -> (String, bool) {
    let (title, _) = tool_title_and_status(text);
    let args = shell_words::split(title).unwrap_or_default();
    let script = if args.first().is_some_and(|arg| {
        matches!(
            Path::new(arg).file_name().and_then(|name| name.to_str()),
            Some("bash" | "sh")
        )
    }) {
        args.windows(2)
            .find(|pair| matches!(pair[0].as_str(), "-c" | "-lc"))
            .map(|pair| pair[1].as_str())
            .unwrap_or(title)
    } else {
        title
    };
    let steps = shell_steps(script);
    if steps.len() <= 1 {
        return simple_tool_description(script);
    }
    let descriptions = steps
        .iter()
        .map(|step| simple_tool_description(step))
        .collect::<Vec<_>>();
    if descriptions.iter().any(|(_, exploratory)| !exploratory) {
        return (script.to_owned(), false);
    }
    let mut labels = Vec::new();
    for (label, _) in descriptions {
        if label != "Change folder" && !labels.contains(&label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        return ("Inspect folder".into(), true);
    }
    let remaining = labels.len().saturating_sub(2);
    let mut summary = labels.into_iter().take(2).collect::<Vec<_>>().join(" · ");
    if remaining > 0 {
        summary.push_str(&format!(" · {remaining} more"));
    }
    (summary, true)
}

pub(super) fn tool_group_heading(entries: &[ChatEntry]) -> &'static str {
    let has_action = entries
        .iter()
        .any(|entry| !is_approval_review(&entry.text) && !is_generic_tool_title(&entry.text));
    if !has_action && entries.iter().any(|entry| is_approval_review(&entry.text)) {
        "Approval checks"
    } else {
        "Ran"
    }
}

pub(super) fn approval_summary(entries: &[ChatEntry]) -> Option<String> {
    let mut total = 0;
    let mut completed = 0;
    let mut failed = 0;
    let mut pending = 0;
    for entry in entries
        .iter()
        .filter(|entry| is_approval_review(&entry.text))
    {
        total += 1;
        match tool_title_and_status(&entry.text).1 {
            Some("completed") => completed += 1,
            Some("failed") => failed += 1,
            Some("pending" | "in_progress") => pending += 1,
            _ => {}
        }
    }
    if total == 0 {
        return None;
    }
    let noun = if total == 1 { "check" } else { "checks" };
    let status = if failed > 0 {
        format!(" · {failed} failed")
    } else if pending > 0 {
        format!(" · {pending} in progress")
    } else if completed == total {
        " · passed".into()
    } else {
        String::new()
    };
    Some(format!("{total} approval {noun}{status}"))
}
