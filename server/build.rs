use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn scan(root: &Path, path: &Path, files: &mut Vec<(String, PathBuf)>) {
    if !path.exists() {
        return;
    }
    for item in fs::read_dir(path).unwrap() {
        let path = item.unwrap().path();
        if path.is_dir() {
            scan(root, &path, files);
        } else if path.is_file() {
            files.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                path,
            ));
        }
    }
}
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../gui/frontend/build");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    scan(&root, &root, &mut files);
    files.sort();
    let mut code = String::from("static ASSETS: &[(&str, &[u8])] = &[\n");
    for (name, path) in files {
        code.push_str(&format!(
            "({name:?}, include_bytes!({:?})),\n",
            path.to_string_lossy()
        ));
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("assets.rs"),
        code,
    )
    .unwrap();
}
