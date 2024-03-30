use std::{fs, path::{PathBuf, Path}, collections::HashMap};

use tracing::{info, instrument};

include!("includes.rs");

#[derive(Clone)]
pub enum Language {
    Rust,
    Python,
    Javascript,
}

#[derive(Clone)]
pub enum Template {
    Chat,
    Echo,
    Fibonacci,
    FileTransfer,
}

impl Language {
    fn to_string(&self) -> String {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::Javascript => "javascript",
        }.to_string()
    }
}

impl Template {
    fn to_string(&self) -> String {
        match self {
            Template::Chat => "chat",
            Template::Echo => "echo",
            Template::Fibonacci => "fibonacci",
            Template::FileTransfer => "file_transfer",
        }.to_string()
    }
}

impl From<&String> for Language {
    fn from(s: &String) -> Self {
        match s.as_str() {
            "rust" => Language::Rust,
            "python" => Language::Python,
            "javascript" => Language::Javascript,
            _ => panic!("kit: language must be 'rust' or 'python'; not '{s}'"),
        }
    }
}

impl From<&String> for Template {
    fn from(s: &String) -> Self {
        match s.as_str() {
            "chat" => Template::Chat,
            "echo" => Template::Echo,
            "fibonacci" => Template::Fibonacci,
            "file_transfer" => Template::FileTransfer,
            _ => panic!("kit: template must be 'chat', 'echo', or 'fibonacci'; not '{s}'"),
        }
    }
}

fn replace_vars(input: &str, package_name: &str, publisher: &str) -> String {
    input
        .replace("{package_name}", package_name)
        .replace("{publisher}", publisher)
        .replace("Cargo.toml_", "Cargo.toml")
        .to_string()
}

fn is_url_safe(input: &str) -> bool {
    let re = regex::Regex::new(r"^[a-zA-Z0-9\-_.~]+$").unwrap();
    re.is_match(input)
}

#[instrument(level = "trace", err, skip_all)]
pub fn execute(
    new_dir: PathBuf,
    package_name: Option<String>,
    publisher: String,
    language: Language,
    template: Template,
    ui: bool,
) -> anyhow::Result<()> {
    // Check if the directory already exists
    if new_dir.exists() {
        let error = format!(
            "Directory {:?} already exists. `kit new` creates a new directory to place the template in. Either remove given directory or provide a non-existing directory to create.",
            new_dir,
        );
        return Err(anyhow::anyhow!(error));
    }

    let (package_name, is_from_dir) = match package_name {
        Some(pn) => (pn, false),
        None => (new_dir.file_name().unwrap().to_str().unwrap().to_string(), true),
    };

    if !is_url_safe(&package_name) {
        let error =
            if !is_from_dir {
                anyhow::anyhow!("`package_name` '{}' must be URL safe.", package_name)
            } else {
                anyhow::anyhow!(
                    "`package_name` (derived from given directory {:?}) '{}' must be URL safe.",
                    new_dir,
                    package_name,
                )
            };
        return Err(error);
    }
    if !is_url_safe(&publisher) {
        return Err(anyhow::anyhow!("`publisher` '{}' must be URL safe.", publisher));
    }

    match language {
        Language::Rust => {
            if package_name.contains('-') {
                let error =
                    if !is_from_dir {
                        anyhow::anyhow!(
                            "rust `package_name`s cannot contain `-`s (given '{}')",
                            package_name,
                        )
                    } else {
                        anyhow::anyhow!(
                            "rust `package_name` (derived from given directory {:?}) cannot contain `-`s (given '{}')",
                            new_dir,
                            package_name,
                        )
                    };
                return Err(error);
            }
        },
        _ => {},
    }

    let ui_infix = if ui { "ui".to_string() } else { "no-ui".to_string() };
    let template_prefix = format!(
        "{}/{}/{}/",
        language.to_string(),
        ui_infix,
        template.to_string(),
    );
    let ui_prefix = format!(
        "{}/{}/",
        ui_infix,
        template.to_string(),
    );
    let mut path_to_content: HashMap<String, Vec<u8>> = PATH_TO_CONTENT
        .iter()
        .filter_map(|(k, v)| {
            k
                .strip_prefix(&template_prefix)
                .or_else(|| k.strip_prefix(&ui_prefix))
                .and_then(|stripped| {
                    let key = replace_vars(stripped, &package_name, &publisher);
                    let val = match std::str::from_utf8(v) {
                        Err(_) => v.to_vec(),
                        Ok(v) => {
                            replace_vars(v, &package_name, &publisher)
                                .as_bytes()
                                .to_vec()
                        },
                    };
                    Some((key, val))
                })
        })
        .collect();

    if path_to_content.is_empty() {
        return Err(anyhow::anyhow!(
            "The {}/{}/{} language/template/ui combination isn't available. See {} for available language/template/ui combinations.",
            language.to_string(),
            template.to_string(),
            if ui { "'yes ui'" } else { "'no ui'" },
            "https://book.kinode.org/kit/new.html#existshas-ui-enabled-vesion",
        ));
    }

    if ui && path_to_content.keys().filter(|p| !p.starts_with("ui")).count() == 0 {
        // Only `ui/` exists
        return Err(anyhow::anyhow!(
            "kit new: cannot use `--ui` for {} {}; template does not exist",
            language.to_string(),
            template.to_string(),
        ));
    }

    // add componentize.mjs
    match language {
        Language::Javascript => {
            path_to_content.insert(
                format!("{}/{}", package_name, PATH_TO_CONTENT[0].0),
                replace_vars(
                    std::str::from_utf8(PATH_TO_CONTENT[0].1).unwrap(),
                    &package_name,
                    &publisher,
                ).as_bytes().to_vec(),
            );
        },
        _ => {},
    }

    // Create the template directory and subdirectories
    path_to_content
        .keys()
        .filter_map(|p| Path::new(p).parent())
        .try_for_each(|p| fs::create_dir_all(new_dir.join(p)))?;

    // Copy the template files
    for (path, content) in path_to_content {
        fs::write(new_dir.join(path), content)?;
    }

    info!("Template directory created successfully at {:?}.", new_dir);
    Ok(())
}
