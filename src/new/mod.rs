use std::{fs, path::{PathBuf, Path}, collections::HashMap};

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
    Fibonacci,
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
            Template::Fibonacci => "fibonacci",
        }.to_string()
    }
}

impl From<&String> for Language {
    fn from(s: &String) -> Self {
        match s.as_str() {
            "rust" => Language::Rust,
            "python" => Language::Python,
            "javascript" => Language::Javascript,
            _ => panic!("uqdev: language must be 'rust' or 'python'; not '{s}'"),
        }
    }
}

impl From<&String> for Template {
    fn from(s: &String) -> Self {
        match s.as_str() {
            "chat" => Template::Chat,
            "fibonacci" => Template::Fibonacci,
            _ => panic!("uqdev: template must be 'chat'; not '{s}'"),
        }
    }
}

fn replace_vars(input: &str, package_name: &str, publisher: &str) -> String {
    input
        .replace("{package_name}", package_name)
        .replace("{publisher}", publisher)
        .to_string()
}

pub fn execute(
    new_dir: PathBuf,
    package_name: String,
    publisher: String,
    language: Language,
    template: Template,
    ui: bool,
) -> anyhow::Result<()> {
    // Check if the directory already exists
    if new_dir.exists() {
        let error = format!(
            "Directory {:?} already exists. Remove it to create a new template.",
            new_dir,
        );
        println!("{}", error);
        return Err(anyhow::anyhow!(error));
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
    let mut path_to_content: HashMap<String, String> = PATH_TO_CONTENT
        .iter()
        .filter_map(|(k, v)| {
            k
                .strip_prefix(&template_prefix)
                .or_else(|| k.strip_prefix(&ui_prefix))
                .and_then(|stripped| {
                    let key = replace_vars(stripped, &package_name, &publisher);
                    let val = replace_vars(v, &package_name, &publisher);
                    Some((key, val))
                })
        })
        .collect();
    if ui && path_to_content.keys().filter(|p| !p.starts_with("ui")).count() == 0 {
        // Only `ui/` exists
        return Err(anyhow::anyhow!(
            "uqdev new: cannot use `--ui` for {} {}; template does not exist",
            language.to_string(),
            template.to_string(),
        ));
    }
    match language {
        Language::Javascript => {
            path_to_content.insert(
                format!("{}/{}", package_name, PATH_TO_CONTENT[0].0),
                replace_vars(PATH_TO_CONTENT[0].1, &package_name, &publisher),
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

    println!("Template directory created successfully at {:?}.", new_dir);
    Ok(())
}
