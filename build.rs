fn main() {
    for path in ["assets/app.rc", "assets/audeze.ico"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let number = version.split('-').next().unwrap().replace('.', ",") + ",0";
    // A generated RC avoids differing command-line quote handling in rc/windres.
    let icon = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("assets/audeze.ico")
        .display()
        .to_string()
        .replace('\\', "/");
    let resource = std::fs::read_to_string("assets/app.rc")
        .unwrap()
        .replace("APP_VERSION_NUM", &number)
        .replace("APP_VERSION", &format!("\"{version}\""))
        .replace("\"audeze.ico\"", &format!("\"{icon}\""));
    let path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("app.rc");
    std::fs::write(&path, resource).unwrap();
    embed_resource::compile_for(path, ["audeze-overlay"], embed_resource::NONE)
        .manifest_required()
        .expect("compile Windows application icon and version information");
}
