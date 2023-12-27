use std::{fs, path::{PathBuf, Path}, collections::HashMap};

include!("includes.rs");

#[derive(Clone)]
pub enum Language {
    Rust,
    Python,
}

#[derive(Clone)]
pub enum Template {
    Chat,
}

impl Language {
    fn to_string(&self) -> String {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
        }.to_string()
    }
}

impl Template {
    fn to_string(&self) -> String {
        match self {
            Template::Chat => "chat",
        }.to_string()
    }
}

impl From<&String> for Language {
    fn from(s: &String) -> Self {
        match s.as_str() {
            "rust" => Language::Rust,
            "python" => Language::Python,
            _ => panic!("uqdev: language must be 'rust' or 'python'; not '{s}'"),
        }
    }
}

impl From<&String> for Template {
    fn from(s: &String) -> Self {
        match s.as_str() {
            "chat" => Template::Chat,
            _ => panic!("uqdev: template must be 'chat'; not '{s}'"),
        }
    }
}

pub fn execute(
    new_dir: PathBuf,
    package_name: String,
    publisher: String,
    language: Language,
    template: Template,
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

    let template_prefix = format!("{}/{}/", language.to_string(), template.to_string());
    let path_to_content: HashMap<String, String> = PATH_TO_CONTENT
        .iter()
        .filter_map(|(k, v)| {
            k
                .strip_prefix(&template_prefix)
                .and_then(|stripped| {
                    let key = stripped
                        .replace("{package_name}", &package_name)
                        .to_string();
                    let val = v
                        .replace("{package_name}", &package_name)
                        .replace("{publisher}", &publisher)
                        .to_string();
                    Some((key, val))
                })
        })
        .collect();

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
