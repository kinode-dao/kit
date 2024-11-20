use std::collections::HashMap;
use std::path::{Path, PathBuf};

use color_eyre::{eyre::eyre, Result};
use fs_err as fs;
use regex::Regex;
use toml_edit;
use tracing::{debug, instrument};

#[derive(Debug, Default)]
struct GeneratedProcesses {
    // original process name -> (generated process name -> (wasm path, content))
    processes: HashMap<String, HashMap<String, (String, String)>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct GeneratedProcessesExternal {
    // original process name -> (generated process name -> wasm path)
    processes: HashMap<String, HashMap<String, String>>,
}

impl From<GeneratedProcesses> for GeneratedProcessesExternal {
    fn from(input: GeneratedProcesses) -> Self {
        let processes = input
            .processes
            .iter()
            .map(|(parent_name, child_to_content)| {
                (
                    parent_name.to_string(),
                    child_to_content
                        .iter()
                        .map(|(child_name, (path, _content))| {
                            (child_name.to_string(), path.to_string())
                        })
                        .collect(),
                )
            })
            .collect();
        GeneratedProcessesExternal { processes }
    }
}

#[derive(Debug)]
struct SpawnMatch {
    args: String,
    body: String,
    imports: Vec<String>,
    start_pos: usize,
    end_pos: usize,
}

#[derive(Debug)]
struct SpawnInfo {
    args: String,         // The arguments passed to the spawn closure
    body: String,         // The body of the spawn closure
    imports: Vec<String>, // All imports from the original file
    wit_bindgen: String,  // `wit_bindgen!()` call
}

#[derive(Debug, thiserror::Error)]
enum SpawnParseError {
    #[error("Parse failed due to malformed imports")]
    Imports,
    #[error("Spawn parse failed due to malformed closure: no closing pipe in closure")]
    NoClosingPipe,
    #[error("Spawn parse failed due to malformed closure: no opening brace")]
    NoOpeningBrace,
    #[error("Spawn parse failed due to malformed closure: unclosed brace")]
    UnclosedBrace,
    #[error("Spawn parse failed due to malformed closure: unclosed paren")]
    UnclosedParen,
}

fn extract_imports(content: &str) -> Result<Vec<String>, SpawnParseError> {
    let imports_re = Regex::new(r"use\s+([^;]+);").map_err(|_| SpawnParseError::Imports)?;
    Ok(imports_re
        .captures_iter(content)
        .map(|cap| cap[1].trim().to_string())
        .collect())
}

fn extract_wit_bindgen(content: &str) -> Option<String> {
    // Look for wit_bindgen::generate! macro
    if let Some(start) = content.find("wit_bindgen::generate!") {
        let mut brace_count = 0;
        let mut in_macro = false;
        let mut saw_closing_brace = false;
        let mut saw_closing_paren = false;
        let mut macro_end = start;

        // Find the closing part of the macro by counting braces
        for (i, c) in content[start..].chars().enumerate() {
            match c {
                '{' => {
                    brace_count += 1;
                    in_macro = true;
                }
                '}' => {
                    brace_count -= 1;
                    if in_macro && brace_count == 0 {
                        saw_closing_brace = true;
                    }
                }
                ')' => {
                    if in_macro && saw_closing_brace && brace_count == 0 {
                        saw_closing_paren = true;
                    }
                }
                ';' => {
                    if in_macro && saw_closing_brace && saw_closing_paren && brace_count == 0 {
                        macro_end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        Some(content[start..macro_end].to_string())
    } else {
        None
    }
}

fn parse_spawn_from(input: &str) -> Result<(String, String, usize), SpawnParseError> {
    // Skip the "Spawn(|" prefix since we know it's there
    let input_after_spawn = &input["Spawn(|".len()..];

    // Find the closing "|"
    let pipe_end = input_after_spawn
        .find('|')
        .ok_or(SpawnParseError::NoClosingPipe)?;

    // Find the opening "{"
    let brace_start = input_after_spawn[pipe_end..]
        .find('{')
        .ok_or(SpawnParseError::NoOpeningBrace)?
        .saturating_add(pipe_end);

    // Find the closing "}" while handling nested braces
    let mut brace_count = 1;
    let mut brace_end = None;
    let mut paren_end = None;

    for (i, c) in input_after_spawn[brace_start + 1..].chars().enumerate() {
        match c {
            '{' => brace_count += 1,
            '}' => {
                brace_count -= 1;
                if brace_count == 0 {
                    brace_end = Some(brace_start + 1 + i);
                }
            }
            ')' => {
                if brace_count == 0 && brace_end.is_some() {
                    paren_end = Some(brace_start + 1 + i);
                    break;
                }
            }
            _ => {}
        }
    }

    let brace_end = brace_end.ok_or(SpawnParseError::UnclosedBrace)?;
    let paren_end = paren_end.ok_or(SpawnParseError::UnclosedParen)?;

    let args = input_after_spawn[..pipe_end].trim().to_string();
    let body = input_after_spawn[brace_start + 1..brace_end]
        .trim()
        .to_string();

    // Return the total length consumed so we know where to continue searching
    let total_consumed = "Spawn(|".len() + paren_end + 1;

    Ok((args, body, total_consumed))
}

fn find_all_spawns(input: &str) -> Result<Vec<SpawnMatch>, SpawnParseError> {
    let mut results = Vec::new();
    let mut search_from = 0;
    let imports = extract_imports(input)?;

    while let Some(spawn_start) = input[search_from..].find("Spawn(|") {
        let absolute_start = search_from + spawn_start;

        let (args, body, consumed_len) = parse_spawn_from(&input[absolute_start..])?;

        results.push(SpawnMatch {
            args,
            body,
            imports: imports.clone(),
            start_pos: absolute_start,
            end_pos: absolute_start + consumed_len,
        });

        search_from = absolute_start + consumed_len;
    }

    Ok(results)
}

#[instrument(level = "trace", skip_all)]
fn generate_worker_process(process_name: &str, spawn_info: &SpawnInfo) -> Result<String> {
    let template = format!(
        r#"// Generated worker process for {process_name}
{}

{}

call_init!(init);
fn init(our: Address) {{
    // Get args from parent
    let message = await_message().expect("Failed to get args from parent");
    let args: serde_json::Value = serde_json::from_slice(&message.body()).unwrap();

    // Execute original spawn body
    {}

    // Exit after completion
    std::process::exit(0);
}}
"#,
        // Add all the original imports
        spawn_info
            .imports
            .iter()
            .map(|i| format!("use {i};\n"))
            .collect::<String>(),
        spawn_info.wit_bindgen,
        spawn_info.body
    );

    Ok(template)
}

#[instrument(level = "trace", skip_all)]
pub fn copy_and_rewrite_package(package_dir: &Path) -> Result<PathBuf> {
    debug!("Rewriting for {}...", package_dir.display());
    let rewrite_dir = package_dir.join("target").join("rewrite");
    if rewrite_dir.exists() {
        fs::remove_dir_all(&rewrite_dir)?;
    }
    fs::create_dir_all(&rewrite_dir)?;

    copy_dir(package_dir, &rewrite_dir)?;

    let mut generated = GeneratedProcesses::default();

    // Process all Rust files in the copied directory
    process_package(&rewrite_dir, &mut generated)?;

    // Create child processes
    create_child_processes(&rewrite_dir, &generated)?;

    // Update workspace Cargo.toml
    update_workspace_cargo_toml(&rewrite_dir, &generated)?;

    Ok(rewrite_dir)
}

// TODO: factor out with build::mod.rs::copy_dir()
#[instrument(level = "trace", skip_all)]
fn copy_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            if src_path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            copy_dir(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn create_child_processes(package_dir: &Path, generated: &GeneratedProcesses) -> Result<()> {
    for (process_name, workers) in &generated.processes {
        for (worker_name, (_, content)) in workers {
            let parent_dir = package_dir.join(process_name);
            let worker_dir = package_dir.join(worker_name);

            // Copy the source directory structure from parent
            let parent_src = parent_dir.join("src");
            let worker_src = worker_dir.join("src");
            debug!("{} {}", parent_src.display(), worker_src.display());
            copy_dir(&parent_src, &worker_src)?;

            // Overwrite lib.rs with our generated content
            fs::write(worker_src.join("lib.rs"), content)?;

            // Copy and modify Cargo.toml
            let parent_cargo = fs::read_to_string(parent_dir.join("Cargo.toml"))?;
            let mut doc = parent_cargo.parse::<toml_edit::DocumentMut>()?;

            // Update package name to worker name
            if let Some(package) = doc.get_mut("package") {
                if let Some(name) = package.get_mut("name") {
                    *name = toml_edit::value(worker_name.as_str());
                }
            }

            fs::write(worker_dir.join("Cargo.toml"), doc.to_string())?;
        }
    }
    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn update_workspace_cargo_toml(package_dir: &Path, generated: &GeneratedProcesses) -> Result<()> {
    let cargo_toml_path = package_dir.join("Cargo.toml");
    let cargo_toml = fs::read_to_string(&cargo_toml_path)?;

    // Parse existing TOML
    let mut doc = cargo_toml.parse::<toml_edit::DocumentMut>()?;

    // Get or create workspace section
    let workspace = doc.entry("workspace").or_insert(toml_edit::table());

    // Get or create members array
    let members = workspace
        .as_table_mut()
        .ok_or_else(|| eyre!("workspace is not a table"))?
        .entry("members")
        .or_insert(toml_edit::array());

    let members_array = members
        .as_array_mut()
        .ok_or_else(|| eyre!("members is not an array"))?;

    // Add all worker packages
    for workers in generated.processes.values() {
        for worker_name in workers.keys() {
            members_array.push(worker_name);
        }
    }

    // Write back to file
    fs::write(cargo_toml_path, doc.to_string())?;

    Ok(())
}

#[instrument(level = "trace", skip_all)]
fn rewrite_rust_file(
    process_name: &str,
    file_name: &str,
    content: &str,
    generated: &mut GeneratedProcesses,
) -> Result<String> {
    let spawn_matches = find_all_spawns(content)?;
    let mut new_content = content.to_string();

    // Process spawns in reverse order to not invalidate positions
    for (i, spawn_match) in spawn_matches.iter().enumerate().rev() {
        let worker_name = format!("{process_name}-worker-{i}");
        let wasm_name = format!("{worker_name}.wasm");

        // Generate worker process
        let wit_bindgen = extract_wit_bindgen(content).unwrap_or_else(|| {
            // Fallback to default if not found
            r#"wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
})"#
            .to_string()
        });

        let worker_code = generate_worker_process(
            file_name,
            &SpawnInfo {
                args: spawn_match.args.clone(),
                body: spawn_match.body.clone(),
                imports: spawn_match.imports.clone(),
                wit_bindgen,
            },
        )?;

        // Track in generated processes
        generated
            .processes
            .entry(process_name.to_string())
            .or_default()
            .insert(worker_name.clone(), (wasm_name, worker_code));

        // Create replacement spawn code
        let args = spawn_match
            .args
            .split(", ")
            .map(|s| format!("\"{s}\":{s}"))
            .collect::<Vec<String>>()
            .join(",");
        let args = "{".to_string() + &args;
        let args = args + "}";
        let replacement = format!(
            r#"{{
        use kinode_process_lib::{{spawn, OnExit, Request}};
        let worker = spawn(
            None,
            &format!("{{}}:{{}}/pkg/{}.wasm", our.process.package_name, our.process.publisher_node),
            OnExit::None,
            vec![],
            vec![],
            false,
        ).expect("failed to spawn worker");
        Request::to((our.node(), worker))
            .body(serde_json::to_vec(&serde_json::json!({})).unwrap())
            .send()
            .expect("failed to initialize worker");
    }}"#,
            worker_name, args,
        );

        // Replace in the content using positions
        new_content.replace_range(spawn_match.start_pos..spawn_match.end_pos, &replacement);
    }

    Ok(new_content)
}

#[instrument(level = "trace", skip_all)]
fn process_package(package_dir: &Path, generated: &mut GeneratedProcesses) -> Result<()> {
    if !package_dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(package_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            process_package(&path, generated)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let process_name = path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|n| n.file_name())
                .and_then(|n| n.to_str())
                .ok_or_else(|| eyre!("Invalid process name"))?
                .to_string();

            let file_name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .ok_or_else(|| eyre!("Invalid file name"))?
                .to_string();

            let content = fs::read_to_string(&path)?;
            let new_content = rewrite_rust_file(&process_name, &file_name, &content, generated)?;
            fs::write(&path, new_content)?;
        }
    }
    Ok(())
}