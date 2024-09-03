use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use color_eyre::{eyre::eyre, Result};
use fs_err as fs;
use tracing::instrument;

include!("../../target/includes.rs");

#[derive(Clone)]
pub enum Language {
    Rust,
    Python,
    Javascript,
}

#[derive(Clone)]
pub enum Template {
    Blank,
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
        }
        .to_string()
    }
}

impl Template {
    fn to_string(&self) -> String {
        match self {
            Template::Blank => "blank",
            Template::Chat => "chat",
            Template::Echo => "echo",
            Template::Fibonacci => "fibonacci",
            Template::FileTransfer => "file_transfer",
        }
        .to_string()
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
            "blank" => Template::Blank,
            "chat" => Template::Chat,
            "echo" => Template::Echo,
            "fibonacci" => Template::Fibonacci,
            "file_transfer" => Template::FileTransfer,
            _ => panic!("kit: template must be 'blank', 'chat', 'echo', or 'fibonacci'; not '{s}'"),
        }
    }
}

fn snake_to_upper_camel_case(input: &str) -> String {
    let parts: Vec<&str> = input.split('_').collect();
    let mut camel_case = String::new();

    for part in parts {
        if let Some(first_char) = part.chars().next() {
            camel_case.push_str(&first_char.to_uppercase().to_string());
            camel_case.push_str(&part[first_char.len_utf8()..]);
        }
    }
    camel_case
}

fn replace_dots(input: &str) -> (String, String) {
    let dotted = input.split('.');
    if dotted.clone().count() == 1 {
        (input.to_string(), input.to_string())
    } else {
        let dotted_snake = dotted.clone().fold(String::new(), |mut d, item| {
            if !d.is_empty() {
                d.push_str("_dot_");
            }
            d.push_str(item);
            d
        });
        let dotted_kebab = dotted.fold(String::new(), |mut d, item| {
            if !d.is_empty() {
                d.push_str("-dot-");
            }
            d.push_str(item);
            d
        });
        (dotted_snake, dotted_kebab)
    }
}

fn replace_vars(
    input: &str,
    template_package_name: &str,
    package_name: &str,
    publisher: &str,
    extension: &str,
) -> String {
    let template_package_name_kebab = template_package_name.replace("_", "-");
    let template_package_name_snake = template_package_name.replace("-", "_");
    let template_package_name_upper_camel = snake_to_upper_camel_case(&template_package_name_snake);

    let package_name_kebab = package_name.replace("_", "-");
    let package_name_snake = package_name.replace("-", "_");
    let package_name_upper_camel = snake_to_upper_camel_case(&package_name_snake);

    let (publisher_dotted_snake, publisher_dotted_kebab) = replace_dots(publisher);
    let publisher_dotted_upper_camel = snake_to_upper_camel_case(&publisher_dotted_snake);
    let input = input
        // wit
        .replace(
            &format!("{template_package_name_kebab}-"),
            &format!("{package_name_kebab}-"),
        )
        // rust imports
        .replace(
            &format!("{template_package_name_snake}::"),
            &format!("{package_name_snake}::"),
        );
    let input = if extension == "wit" {
        input
            .replace(
                &format!("{template_package_name}-"),
                &format!("{package_name_kebab}-"),
            )
            .replace(&template_package_name_kebab, &package_name_kebab)
            .replace(template_package_name, package_name)
    } else {
        input
            .replace(
                &format!("{template_package_name}-"),
                &format!("{package_name_kebab}-"),
            )
            .replace(template_package_name, package_name)
            .replace(&template_package_name_kebab, &package_name_kebab)
    };
    input
        .replace(
            &template_package_name_upper_camel,
            &package_name_upper_camel,
        )
        .replace("template.os", publisher)
        .replace("template_dot_os", &publisher_dotted_snake)
        .replace("template-dot-os", &publisher_dotted_kebab)
        .replace("TemplateDotOs", &publisher_dotted_upper_camel)
        .to_string()
}

fn is_url_safe(input: &str) -> bool {
    let re = regex::Regex::new(r"^[a-zA-Z0-9\-_.~]+$").unwrap();
    re.is_match(input)
}

#[instrument(level = "trace", skip_all)]
pub fn execute(
    new_dir: PathBuf,
    package_name: Option<String>,
    publisher: String,
    language: Language,
    template: Template,
    ui: bool,
) -> Result<()> {
    // Check if the directory already exists
    if new_dir.exists() {
        let error = format!(
            "Directory {:?} already exists. `kit new` creates a new directory to place the template in. Either remove given directory or provide a non-existing directory to create.",
            new_dir,
        );
        return Err(eyre!(error));
    }

    let (package_name, is_from_dir) = match package_name {
        Some(pn) => (pn, false),
        None => (
            new_dir.file_name().unwrap().to_str().unwrap().to_string(),
            true,
        ),
    };

    let disallowed_package_names = HashSet::from(["api", "test"]);
    if disallowed_package_names.contains(package_name.as_str()) {
        return Err(eyre!(
            "Package name {} not allowed; cannot be in {:?}.",
            package_name,
            disallowed_package_names,
        ));
    }

    if !is_url_safe(&package_name) {
        let error = if !is_from_dir {
            eyre!("`package_name` '{}' must be URL safe.", package_name)
        } else {
            eyre!(
                "`package_name` (derived from given directory {:?}) '{}' must be URL safe.",
                new_dir,
                package_name,
            )
        };
        return Err(error);
    }
    if !is_url_safe(&publisher) {
        return Err(eyre!("`publisher` '{}' must be URL safe.", publisher));
    }

    // match language {
    //     Language::Rust => {
    //         if package_name.contains('-') {
    //             let error = if !is_from_dir {
    //                 eyre!(
    //                     "rust `package_name`s cannot contain `-`s (given '{}')",
    //                     package_name,
    //                 )
    //             } else {
    //                 eyre!(
    //                         "rust `package_name` (derived from given directory {:?}) cannot contain `-`s (given '{}')",
    //                         new_dir,
    //                         package_name,
    //                     )
    //             };
    //             return Err(error);
    //         }
    //     }
    //     _ => {}
    // }

    let ui_infix = if ui {
        "ui".to_string()
    } else {
        "no-ui".to_string()
    };
    let template_prefix = format!(
        "{}/{}/{}/",
        language.to_string(),
        ui_infix,
        template.to_string(),
    );
    let ui_prefix = format!("{}/{}/", ui_infix, template.to_string());
    let test_prefix = format!("test/{}/", template.to_string());
    let mut path_to_content: HashMap<String, String> = PATH_TO_CONTENT
        .iter()
        .filter_map(|(path, content)| {
            path.strip_prefix(&template_prefix)
                .map(|p| p.to_string())
                .or_else(|| path.strip_prefix(&ui_prefix).map(|p| p.to_string()))
                .or_else(|| {
                    if path.starts_with(&test_prefix) {
                        Some(path.to_string())
                    } else {
                        None
                    }
                })
                .and_then(|stripped| {
                    let extension = PathBuf::from(path);
                    let extension = extension
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default();
                    let modified_path = replace_vars(
                        &stripped,
                        &template.to_string(),
                        &package_name,
                        &publisher,
                        extension,
                    );
                    let modified_content = replace_vars(
                        content,
                        &template.to_string(),
                        &package_name,
                        &publisher,
                        extension,
                    );
                    Some((modified_path, modified_content))
                })
        })
        .collect();

    if path_to_content.is_empty() {
        return Err(eyre!(
            "The {}/{}/{} language/template/ui combination isn't available. See {} for available language/template/ui combinations.",
            language.to_string(),
            template.to_string(),
            if ui { "'yes ui'" } else { "'no ui'" },
            "https://book.kinode.org/kit/new.html#existshas-ui-enabled-vesion",
        ));
    }

    if ui
        && path_to_content
            .keys()
            .filter(|p| !p.starts_with("ui"))
            .count()
            == 0
    {
        // Only `ui/` exists
        return Err(eyre!(
            "kit new: cannot use `--ui` for {} {}; template does not exist",
            language.to_string(),
            template.to_string(),
        ));
    }
    match language {
        Language::Javascript => {
            path_to_content.insert(
                format!("{}/{}", package_name, PATH_TO_CONTENT[0].0),
                replace_vars(
                    PATH_TO_CONTENT[0].1,
                    &template.to_string(),
                    &package_name,
                    &publisher,
                    "js",
                ),
            );
        }
        _ => {}
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

    tracing::info!("Template directory created successfully at {:?}.", new_dir);
    Ok(())
}
