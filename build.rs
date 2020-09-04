extern crate toml;

use serde_derive::Deserialize;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::Read;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Cargo {
    package: CargoPackage,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: String,
    description: String,
    version: String,
}

fn main() {
    let cargo = read_cargo();
    compile_resources(&cargo);
}

fn read_cargo() -> Cargo {
    let mut toml = String::new();
    File::open(PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml"))
        .unwrap()
        .read_to_string(&mut toml)
        .unwrap();
    toml::from_str::<Cargo>(&toml).unwrap()
}

fn compile_resources(cargo: &Cargo) {
    let icon =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/icon/terminal.ico");
    let manifest_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("manifest.xml");
    let mut f = File::create(manifest_path.clone()).unwrap();
    f.write_all(get_manifest(cargo).as_bytes()).unwrap();
    winres::WindowsResource::new()
        .set_manifest_file(manifest_path.to_str().unwrap())
        .set_icon_with_id(icon.to_str().unwrap(), "app")
        .set("InternalName", "wslscript.exe")
        .set("LegalCopyright", "Joni Eskelinen © 2019")
        .compile()
        .unwrap();
}

fn get_manifest(cargo: &Cargo) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1"
    manifestVersion="1.0">
    <assemblyIdentity version="{version}"
        name="{name}"
        type="win32" />
    <description>{description}</description>
    <dependency>
        <dependentAssembly>
            <assemblyIdentity type="win32"
                name="Microsoft.Windows.Common-Controls"
                version="6.0.0.0"
                processorArchitecture="*"
                publicKeyToken="6595b64144ccf1df"
                language="*" />
        </dependentAssembly>
    </dependency>
</assembly>"#,
        name = format!("github.sop.{}", cargo.package.name),
        description = cargo.package.description,
        version = format!("{}.0", cargo.package.version)
    )
}
