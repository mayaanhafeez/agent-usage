use std::{env, path::Path};

use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::model::Provider;

fn provider_from_text(text: &str) -> Option<Provider> {
    let lower = text.to_ascii_lowercase();
    Provider::ALL.into_iter().find(|provider| {
        provider.process_names().iter().any(|name| {
            lower == *name
                || lower.ends_with(&format!("/{name}"))
                || lower
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .any(|part| part == *name)
        })
    })
}

pub fn detect_all() -> Vec<Provider> {
    if let Ok(value) = env::var("AGENT_USAGE_PROVIDER")
        && let Some(provider) = provider_from_text(&value)
    {
        return vec![provider];
    }

    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let mut active = Vec::new();

    // Include wrapper-based agents whose executable may only be visible in the
    // ancestry command line (for example, a Node-installed Claude CLI).
    let mut pid = Pid::from_u32(std::process::id());
    for _ in 0..12 {
        let Some(process) = system.process(pid) else {
            break;
        };
        let name = process.name().to_string_lossy();
        let exe = process
            .exe()
            .and_then(Path::file_name)
            .map(|s| s.to_string_lossy());
        let cmd = process
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        for candidate in [name.as_ref(), exe.as_deref().unwrap_or(""), cmd.as_str()] {
            if let Some(provider) = provider_from_text(candidate) {
                push_unique(&mut active, provider);
            }
        }
        let Some(parent) = process.parent() else {
            break;
        };
        pid = parent;
    }

    for process in system.processes().values() {
        let name = process.name().to_string_lossy();
        let exe = process
            .exe()
            .and_then(Path::file_name)
            .map(|s| s.to_string_lossy());
        let command = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        let provider = provider_from_text(&name)
            .or_else(|| exe.as_deref().and_then(provider_from_text))
            .or_else(|| wrapper_provider(&command));
        if let Some(provider) = provider {
            push_unique(&mut active, provider);
        }
    }

    // Stable ordering prevents cards from jumping around as processes restart.
    Provider::ALL
        .into_iter()
        .filter(|provider| active.contains(provider))
        .collect()
}

fn wrapper_provider(command: &str) -> Option<Provider> {
    if command.contains("claude-code") || command.contains("/claude/cli") {
        Some(Provider::Claude)
    } else if command.contains("gemini-cli") {
        Some(Provider::Gemini)
    } else {
        None
    }
}

fn push_unique(providers: &mut Vec<Provider>, provider: Provider) {
    if !providers.contains(&provider) {
        providers.push(provider);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_node_wrappers() {
        assert_eq!(
            wrapper_provider("node @anthropic-ai/claude-code/cli.js"),
            Some(Provider::Claude)
        );
        assert_eq!(
            wrapper_provider("node @google/gemini-cli/dist/index.js"),
            Some(Provider::Gemini)
        );
    }
}
