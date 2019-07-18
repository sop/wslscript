use std::env;
use std::path::PathBuf;

fn main() {
    compile_resources();
}

fn compile_resources() {
    let project_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut icon = project_dir;
    icon.push("assets/icon/terminal.ico");
    let mut res = winres::WindowsResource::new();
    res.set_icon_with_id(icon.to_str().unwrap(), "app")
        .set("InternalName", "wslscript.exe")
        .set("LegalCopyright", "Joni Eskelinen © 2019");
    res.compile().unwrap();
}
