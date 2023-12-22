use std::{fs, path::{PathBuf, Path}, collections::HashMap};

const PATH_TO_CONTENT: &[(&str, &str)] = &[
    (".gitignore",                include_str!("templates/rust/chat/.gitignore")),
    ("{package_name}/Cargo.toml", include_str!("templates/rust/chat/chat/Cargo.toml")),
    ("{package_name}/src/lib.rs", include_str!("templates/rust/chat/chat/src/lib.rs")),
    ("pkg/manifest.json",         include_str!("templates/rust/chat/pkg/manifest.json")),
    ("pkg/metadata.json",         include_str!("templates/rust/chat/pkg/metadata.json")),
    ("ui/.eslintrc.cjs",          include_str!("templates/rust/chat/ui/.eslintrc.cjs")),
    ("ui/.gitignore",             include_str!("templates/rust/chat/ui/.gitignore")),
    ("ui/README.md",              include_str!("templates/rust/chat/ui/README.md")),
    ("ui/package.json",           include_str!("templates/rust/chat/ui/package.json")),
    ("ui/package-lock.json",      include_str!("templates/rust/chat/ui/package-lock.json")),
    ("ui/tsconfig.json",          include_str!("templates/rust/chat/ui/tsconfig.json")),
    ("ui/tsconfig.node.json",     include_str!("templates/rust/chat/ui/tsconfig.node.json")),
    ("ui/vite.config.ts",         include_str!("templates/rust/chat/ui/vite.config.ts")),
    ("ui/index.html",             include_str!("templates/rust/chat/ui/index.html")),
    ("ui/public/assets/vite.svg", include_str!("templates/rust/chat/ui/public/assets/vite.svg")),
    ("ui/src/App.css",            include_str!("templates/rust/chat/ui/src/App.css")),
    ("ui/src/App.tsx",            include_str!("templates/rust/chat/ui/src/App.tsx")),
    ("ui/src/assets/react.svg",   include_str!("templates/rust/chat/ui/src/assets/react.svg")),
    ("ui/src/index.css",          include_str!("templates/rust/chat/ui/src/index.css")),
    ("ui/src/main.tsx",           include_str!("templates/rust/chat/ui/src/main.tsx")),
    ("ui/src/store/chat.ts",      include_str!("templates/rust/chat/ui/src/store/chat.ts")),
    ("ui/src/types/Chat.ts",      include_str!("templates/rust/chat/ui/src/types/Chat.ts")),
    ("ui/src/types/global.ts",    include_str!("templates/rust/chat/ui/src/types/global.ts")),
    ("ui/src/vite-env.d.ts",      include_str!("templates/rust/chat/ui/src/vite-env.d.ts")),
];

pub fn execute(new_dir: PathBuf, package_name: String, publisher: String) -> anyhow::Result<()> {
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
    for entry in &[
        ".gitignore",
        "{package_name}/Cargo.toml",
        "{package_name}/src/lib.rs",
        "pkg/manifest.json",
        "pkg/metadata.json",
    ] {
        path_to_content
            .entry(entry.to_string())
            .and_modify(|c| *c = c.replace("{package_name}", &package_name))
            .and_modify(|c| *c = c.replace("{publisher}", &publisher));
    }

    // Create the template directory and subdirectories
    fs::create_dir_all(new_dir.join("pkg"))?;
    fs::create_dir_all(new_dir.join(&package_name).join("src"))?;
    fs::create_dir_all(new_dir.join("ui"))?;
    fs::create_dir_all(new_dir.join("ui/public"))?;
    fs::create_dir_all(new_dir.join("ui/public/assets"))?;
    fs::create_dir_all(new_dir.join("ui/src"))?;
    fs::create_dir_all(new_dir.join("ui/src/assets"))?;
    fs::create_dir_all(new_dir.join("ui/src/store"))?;
    fs::create_dir_all(new_dir.join("ui/src/types"))?;

    // Copy the template files
    for (path, content) in path_to_content {
        let path = path.replace("{package_name}", &package_name);
        fs::write(new_dir.join(path.replace("{package_name}", &package_name)), content)?;
    }

    println!("Template directory created successfully at {:?}.", new_dir);
    Ok(())
}
