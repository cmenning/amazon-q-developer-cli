#[cfg(unix)]
use std::io::{
    BufReader,
    Cursor,
    Write,
    stdout,
};

#[cfg(unix)]
use crossterm::execute;
#[cfg(unix)]
use crossterm::terminal::{
    EnterAlternateScreen,
    LeaveAlternateScreen,
};
use eyre::{
    Result,
    eyre,
};
use rustyline::{
    Cmd,
    ConditionalEventHandler,
    EventContext,
    RepeatCount,
};
#[cfg(unix)]
use skim::prelude::*;
#[cfg(unix)]
use tempfile::NamedTempFile;

use super::context::ContextManager;
use crate::os::Os;
use std::sync::Arc;

#[cfg(unix)]
pub struct SkimCommandSelector {
    os: Os,
    context_manager: Arc<ContextManager>,
    tool_names: Vec<String>,
}

#[cfg(windows)]
pub struct SkimCommandSelector {
    _os: Os,
    _context_manager: Arc<ContextManager>,
    _tool_names: Vec<String>,
}

#[cfg(unix)]
impl SkimCommandSelector {
    /// This allows the ConditionalEventHandler handle function to be bound to a KeyEvent.
    pub fn new(os: Os, context_manager: Arc<ContextManager>, tool_names: Vec<String>) -> Self {
        Self {
            os,
            context_manager,
            tool_names,
        }
    }
}

#[cfg(windows)]
impl SkimCommandSelector {
    pub fn new(os: Os, context_manager: Arc<ContextManager>, tool_names: Vec<String>) -> Self {
        Self {
            _os: os,
            _context_manager: context_manager,
            _tool_names: tool_names,
        }
    }
}

#[cfg(unix)]
impl ConditionalEventHandler for SkimCommandSelector {
    fn handle(&self, _evt: &rustyline::Event, _n: RepeatCount, _positive: bool, _ctx: &EventContext<'_>) -> Option<Cmd> {
        // Launch skim command selector with the context manager if available
        match select_command(&self.os, self.context_manager.as_ref(), &self.tool_names) {
            Ok(Some(command)) => Some(Cmd::Insert(1, command)),
            _ => {
                // If cancelled or error, do nothing
                Some(Cmd::Noop)
            },
        }
    }
}

#[cfg(windows)]
impl ConditionalEventHandler for SkimCommandSelector {
    fn handle(&self, _evt: &rustyline::Event, _n: RepeatCount, _positive: bool, _ctx: &EventContext<'_>) -> Option<Cmd> {
        Some(Cmd::Noop)
    }
}

pub fn get_available_commands(os: &Os) -> Vec<String> {
    // Use the experiment-aware function from prompt.rs
    super::prompt::get_available_commands(os)
        .iter()
        .map(|&cmd| cmd.to_string())
        .collect()
}

#[cfg(unix)]
fn create_skim_options(prompt: &str, multi: bool) -> Result<SkimOptions> {
    SkimOptionsBuilder::default()
        .height("100%".to_string())
        .prompt(prompt.to_string())
        .reverse(true)
        .multi(multi)
        .build()
        .map_err(|e| eyre!("Failed to build skim options: {}", e))
}

#[cfg(unix)]
fn run_skim_with_options(options: &SkimOptions, items: SkimItemReceiver) -> Result<Option<Vec<Arc<dyn SkimItem>>>> {
    // Enter alternate screen to prevent skim output from persisting in terminal history
    execute!(stdout(), EnterAlternateScreen).map_err(|e| eyre!("Failed to enter alternate screen: {}", e))?;

    let selected_items =
        Skim::run_with(options, Some(items)).and_then(|out| if out.is_abort { None } else { Some(out.selected_items) });

    execute!(stdout(), LeaveAlternateScreen).map_err(|e| eyre!("Failed to leave alternate screen: {}", e))?;

    Ok(selected_items)
}

#[cfg(unix)]
fn extract_selections(items: Vec<Arc<dyn SkimItem>>) -> Vec<String> {
    items.iter().map(|item| item.output().to_string()).collect()
}

#[cfg(unix)]
pub fn launch_skim_selector(items: &[String], prompt: &str, multi: bool) -> Result<Option<Vec<String>>> {
    let mut temp_file_for_skim_input = NamedTempFile::new()?;
    temp_file_for_skim_input.write_all(items.join("\n").as_bytes())?;

    let options = create_skim_options(prompt, multi)?;
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(BufReader::new(std::fs::File::open(temp_file_for_skim_input.path())?));

    // Run skim and get selected items
    match run_skim_with_options(&options, items)? {
        Some(items) if !items.is_empty() => {
            let selections = extract_selections(items);
            Ok(Some(selections))
        },
        _ => Ok(None), // User cancelled or no selection
    }
}

#[cfg(unix)]
pub fn select_files_with_skim() -> Result<Option<Vec<String>>> {
    // Create skim options with appropriate settings
    let options = create_skim_options("Select files: ", true)?;

    // Create a command that will be executed by skim
    // This command checks if git is installed and if we're in a git repo
    // Otherwise falls back to find command
    let find_cmd = r#"
    # Check if git is available and we're in a git repo
    if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree &>/dev/null; then
        # Git repository - respect .gitignore
        { git ls-files; git ls-files --others --exclude-standard; } | sort | uniq
    else
        # Not a git repository or git not installed - use find command
        find . -type f -not -path '*/\.*'
    fi
    "#;

    // Create a command collector that will execute the find command
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(BufReader::new(
        std::process::Command::new("sh")
            .args(["-c", find_cmd])
            .stdout(std::process::Stdio::piped())
            .spawn()?
            .stdout
            .ok_or_else(|| eyre!("Failed to get stdout from command"))?,
    ));

    // Run skim with the command output as a stream
    match run_skim_with_options(&options, items)? {
        Some(items) if !items.is_empty() => {
            let selections = extract_selections(items);
            Ok(Some(selections))
        },
        _ => Ok(None), // User cancelled or no selection
    }
}

#[cfg(unix)]
pub fn select_context_paths_with_skim(context_manager: &ContextManager) -> Result<Option<(Vec<String>, bool)>> {
    let mut all_paths = Vec::new();

    // Get profile-specific paths
    for path in &context_manager.paths {
        all_paths.push(format!(
            "(agent: {}) {}",
            context_manager.current_profile,
            path.get_path_as_str()
        ));
    }

    if all_paths.is_empty() {
        return Ok(None); // No paths to select
    }

    // Create skim options
    let options = create_skim_options("Select paths to remove: ", true)?;

    // Create item reader
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(all_paths.join("\n")));

    // Run skim and get selected paths
    match run_skim_with_options(&options, items)? {
        Some(items) if !items.is_empty() => {
            let selected_paths = extract_selections(items);

            // Check if any global paths were selected
            let has_global = selected_paths.iter().any(|p| p.starts_with("(global)"));

            // Extract the actual paths from the formatted strings
            let paths: Vec<String> = selected_paths
                .iter()
                .map(|p| {
                    // Extract the path part after the prefix
                    let parts: Vec<&str> = p.splitn(2, ") ").collect();
                    if parts.len() > 1 {
                        parts[1].to_string()
                    } else {
                        p.clone()
                    }
                })
                .collect();

            Ok(Some((paths, has_global)))
        },
        _ => Ok(None), // User cancelled selection
    }
}

#[cfg(unix)]
pub fn select_command(os: &Os, context_manager: &ContextManager, tools: &[String]) -> Result<Option<String>> {
    let commands = get_available_commands(os);

    match launch_skim_selector(&commands, "Select command: ", false)? {
        Some(selections) if !selections.is_empty() => {
            let selected_command = &selections[0];

            match CommandType::from_str(selected_command) {
                Some(CommandType::ContextAdd(cmd)) => {
                    // For context add commands, we need to select files
                    match select_files_with_skim()? {
                        Some(files) if !files.is_empty() => {
                            // Construct the full command with selected files
                            let mut cmd = cmd.clone();
                            for file in files {
                                cmd.push_str(&format!(" {}", file));
                            }
                            Ok(Some(cmd))
                        },
                        _ => Ok(Some(selected_command.clone())), /* User cancelled file selection, return just the
                                                                  * command */
                    }
                },
                Some(CommandType::ContextRemove(cmd)) => {
                    // For context rm commands, we need to select from existing context paths
                    match select_context_paths_with_skim(context_manager)? {
                        Some((paths, has_global)) if !paths.is_empty() => {
                            // Construct the full command with selected paths
                            let mut full_cmd = cmd.clone();
                            if has_global {
                                full_cmd.push_str(" --global");
                            }
                            for path in paths {
                                full_cmd.push_str(&format!(" {}", path));
                            }
                            Ok(Some(full_cmd))
                        },
                        Some((_, _)) => Ok(Some(format!("{} (No paths selected)", cmd))),
                        None => Ok(Some(selected_command.clone())), // User cancelled path selection
                    }
                },
                Some(CommandType::Tools(_)) => {
                    let options = create_skim_options("Select tool: ", false)?;
                    let item_reader = SkimItemReader::default();
                    let items = item_reader.of_bufread(Cursor::new(tools.join("\n")));
                    let selected_tool = match run_skim_with_options(&options, items)? {
                        Some(items) if !items.is_empty() => Some(items[0].output().to_string()),
                        _ => None,
                    };

                    match selected_tool {
                        Some(tool) => Ok(Some(format!("{} {}", selected_command, tool))),
                        None => Ok(Some(selected_command.clone())), /* User cancelled tool selection, return just the
                                                                     * command */
                    }
                },
                Some(cmd @ CommandType::Agent(_)) if cmd.needs_agent_selection() => {
                    // For profile operations that need a profile name, show profile selector
                    // As part of the agent implementation, we are disabling the ability to
                    // switch profile after a session has started.
                    // TODO: perhaps revive this after we have a decision on profile switching
                    Ok(Some(selected_command.clone()))
                },
                Some(CommandType::Agent(_)) => {
                    // For other profile operations (like create), just return the command
                    Ok(Some(selected_command.clone()))
                },
                None => {
                    // Command doesn't need additional parameters
                    Ok(Some(selected_command.clone()))
                },
            }
        },
        _ => Ok(None), // User cancelled command selection
    }
}

#[cfg(unix)]
#[derive(PartialEq)]
enum CommandType {
    ContextAdd(String),
    ContextRemove(String),
    Tools(&'static str),
    Agent(&'static str),
}

#[cfg(unix)]
impl CommandType {
    fn needs_agent_selection(&self) -> bool {
        matches!(self, CommandType::Agent("set" | "delete" | "rename"))
    }

    fn from_str(cmd: &str) -> Option<CommandType> {
        if cmd.starts_with("/context add") {
            Some(CommandType::ContextAdd(cmd.to_string()))
        } else if cmd.starts_with("/context rm") {
            Some(CommandType::ContextRemove(cmd.to_string()))
        } else {
            match cmd {
                "/tools trust" => Some(CommandType::Tools("trust")),
                "/tools untrust" => Some(CommandType::Tools("untrust")),
                "/agent set" => Some(CommandType::Agent("set")),
                "/agent delete" => Some(CommandType::Agent("delete")),
                "/agent rename" => Some(CommandType::Agent("rename")),
                "/agent create" => Some(CommandType::Agent("create")),
                _ => None,
            }
        }
    }
}

#[cfg(windows)]
pub fn launch_skim_selector(items: &[String], prompt: &str, multi: bool) -> Result<Option<Vec<String>>> {
    use dialoguer::FuzzySelect;
    use dialoguer::MultiSelect;

    if items.is_empty() {
        return Ok(None);
    }

    if multi {
        let selections = MultiSelect::new()
            .with_prompt(prompt)
            .items(items)
            .interact_opt()?;
        
        Ok(selections.map(|indices| indices.iter().map(|&i| items[i].clone()).collect()))
    } else {
        let selection = FuzzySelect::new()
            .with_prompt(prompt)
            .items(items)
            .interact_opt()?;
        
        Ok(selection.map(|i| vec![items[i].clone()]))
    }
}

#[cfg(windows)]
pub fn select_files_with_skim() -> Result<Option<Vec<String>>> {
    use dialoguer::MultiSelect;
    use walkdir::WalkDir;

    let files: Vec<String> = WalkDir::new(".")
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| !e.path().to_string_lossy().contains("\\."))
        .map(|e| e.path().display().to_string())
        .collect();

    if files.is_empty() {
        return Ok(None);
    }

    let selections = MultiSelect::new()
        .with_prompt("Select files: ")
        .items(&files)
        .interact_opt()?;
    
    Ok(selections.map(|indices| indices.iter().map(|&i| files[i].clone()).collect()))
}

#[cfg(windows)]
pub fn select_context_paths_with_skim(context_manager: &ContextManager) -> Result<Option<(Vec<String>, bool)>> {
    use dialoguer::MultiSelect;

    let mut all_paths = Vec::new();

    for path in &context_manager.paths {
        all_paths.push(format!(
            "(agent: {}) {}",
            context_manager.current_profile,
            path.get_path_as_str()
        ));
    }

    if all_paths.is_empty() {
        return Ok(None);
    }

    let selections = MultiSelect::new()
        .with_prompt("Select paths to remove: ")
        .items(&all_paths)
        .interact_opt()?;

    match selections {
        Some(indices) if !indices.is_empty() => {
            let selected_paths: Vec<String> = indices.iter().map(|&i| all_paths[i].clone()).collect();
            let has_global = selected_paths.iter().any(|p| p.starts_with("(global)"));
            
            let paths: Vec<String> = selected_paths
                .iter()
                .map(|p| {
                    let parts: Vec<&str> = p.splitn(2, ") ").collect();
                    if parts.len() > 1 {
                        parts[1].to_string()
                    } else {
                        p.clone()
                    }
                })
                .collect();

            Ok(Some((paths, has_global)))
        },
        _ => Ok(None),
    }
}

#[cfg(windows)]
pub fn select_command(os: &Os, context_manager: &ContextManager, tools: &[String]) -> Result<Option<String>> {
    use dialoguer::FuzzySelect;

    let commands = get_available_commands(os);

    let selection = FuzzySelect::new()
        .with_prompt("Select command: ")
        .items(&commands)
        .interact_opt()?;

    match selection {
        Some(idx) => {
            let selected_command = &commands[idx];

            if selected_command.starts_with("/context add") {
                match select_files_with_skim()? {
                    Some(files) if !files.is_empty() => {
                        let mut cmd = selected_command.clone();
                        for file in files {
                            cmd.push_str(&format!(" {}", file));
                        }
                        Ok(Some(cmd))
                    },
                    _ => Ok(Some(selected_command.clone())),
                }
            } else if selected_command.starts_with("/context rm") {
                match select_context_paths_with_skim(context_manager)? {
                    Some((paths, has_global)) if !paths.is_empty() => {
                        let mut full_cmd = selected_command.clone();
                        if has_global {
                            full_cmd.push_str(" --global");
                        }
                        for path in paths {
                            full_cmd.push_str(&format!(" {}", path));
                        }
                        Ok(Some(full_cmd))
                    },
                    _ => Ok(Some(selected_command.clone())),
                }
            } else if selected_command.starts_with("/tools") {
                let tool_selection = FuzzySelect::new()
                    .with_prompt("Select tool: ")
                    .items(tools)
                    .interact_opt()?;
                
                match tool_selection {
                    Some(tool_idx) => Ok(Some(format!("{} {}", selected_command, tools[tool_idx]))),
                    None => Ok(Some(selected_command.clone())),
                }
            } else {
                Ok(Some(selected_command.clone()))
            }
        },
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// Test to verify that all hardcoded command strings in select_command
    /// are present in the COMMANDS array from prompt.rs
    #[tokio::test]
    async fn test_hardcoded_commands_in_commands_array() {
        // Create a mock Os for testing
        let mock_os = crate::os::Os::new().await.unwrap();

        // Get the set of available commands from prompt.rs
        let available_commands: HashSet<String> = get_available_commands(&mock_os).iter().cloned().collect();

        // List of hardcoded commands used in select_command
        let hardcoded_commands = vec![
            "/context add",
            "/context rm",
            "/tools trust",
            "/tools untrust",
            "/agent set",
            "/agent delete",
            "/agent rename",
            "/agent create",
        ];

        // Check that each hardcoded command is in the COMMANDS array
        for cmd in hardcoded_commands {
            assert!(
                available_commands.contains(cmd),
                "Command '{}' is used in select_command but not defined in COMMANDS array",
                cmd
            );

            // This should assert that all the commands we assert are present in the match statement of
            // select_command()
            assert!(
                CommandType::from_str(cmd).is_some(),
                "Command '{}' cannot be parsed into a CommandType",
                cmd
            );
        }
    }
}
