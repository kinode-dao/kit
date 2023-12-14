use std::{fs, path::PathBuf, collections::HashMap};

const PATH_TO_CONTENT: &[(&str, &str)] = &[
    ("Cargo.toml",        include_str!("templates/rust/chat/Cargo.toml")),
    (".gitignore",        include_str!("templates/rust/chat/.gitignore")),
    ("pkg/manifest.json", include_str!("templates/rust/chat/pkg/manifest.json")),
    ("pkg/metadata.json", include_str!("templates/rust/chat/pkg/metadata.json")),
    ("src/lib.rs",        include_str!("templates/rust/chat/src/lib.rs")),
];

pub fn execute(new_dir: PathBuf, package_name: String) -> anyhow::Result<()> {
    // Check if the directory already exists
    if new_dir.exists() {
        let error = format!(
            "Directory {:?} already exists. Remove it to create a new template.",
            new_dir,
        );
        println!("{}", error);
        return Err(anyhow::anyhow!(error));
    }

    let mut path_to_content: HashMap<String, String> = PATH_TO_CONTENT
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    for entry in &["Cargo.toml", "pkg/manifest.json", "pkg/metadata.json", "src/lib.rs"] {
        path_to_content
            .entry(entry.to_string())
            .and_modify(|c| *c = c.replace("{package_name}", &package_name));
    }

    // Create the template directory and subdirectories
    fs::create_dir_all(new_dir.join("pkg"))?;
    fs::create_dir_all(new_dir.join("src"))?;

    for (path, content) in path_to_content {
        fs::write(new_dir.join(path), content)?;
    }

    println!("Template directory created successfully at {:?}.", new_dir);
    Ok(())
}
