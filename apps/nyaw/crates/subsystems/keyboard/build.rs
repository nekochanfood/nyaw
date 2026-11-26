use std::{env, fs, path::Path};

fn files(dir: &Path, output: &mut String) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, output);
        } else {
            let name = path
                .strip_prefix("data")
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            let absolute = fs::canonicalize(&path).unwrap();
            output.push_str(&format!(
                "({name:?}, include_bytes!({:?})),\n",
                absolute.to_str().unwrap()
            ));
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=data");
    let mut output = String::from("const DATA: &[(&str, &[u8])] = &[\n");
    files(Path::new("data"), &mut output);
    output.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("data.rs"),
        output,
    )
    .unwrap();
}
