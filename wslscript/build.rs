use serde_derive::Deserialize;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::Read;
use std::path::PathBuf;
use winres::VersionInfo;

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
    let icon = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .join("assets/icon/terminal.ico");
    let manifest_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("manifest.xml");
    let mut f = File::create(manifest_path.clone()).unwrap();
    f.write_all(get_manifest(&cargo).as_bytes()).unwrap();
    let now = chrono::Local::now();
    let version = parse_version(&cargo.package.version);
    winres::WindowsResource::new()
        .set_manifest_file(manifest_path.to_str().unwrap())
        .set_icon_with_id(icon.to_str().unwrap(), "app")
        .set("ProductName", "WSL Script")
        .set("FileDescription", &cargo.package.description)
        .set("FileVersion", &cargo.package.version)
        .set_version_info(VersionInfo::FILEVERSION, version)
        .set("ProductVersion", &cargo.package.version)
        .set_version_info(VersionInfo::PRODUCTVERSION, version)
        .set("InternalName", &format!("{}.exe", cargo.package.name))
        .set(
            "LegalCopyright",
            &format!("Joni Eskelinen © {}", now.format("%Y")),
        )
        .compile()
        .unwrap();
}

fn parse_version(s: &str) -> u64 {
    let mut parts = s
        .split(".")
        .map(|s| s.parse::<u16>().unwrap())
        .collect::<Vec<_>>();
    parts.push(0);
    assert!(parts.len() == 4);
    (parts[0] as u64) << 48 | (parts[1] as u64) << 32 | (parts[2] as u64) << 16 | (parts[3] as u64)
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

fn read_cargo() -> Cargo {
    let mut toml = String::new();
    File::open(PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml"))
        .unwrap()
        .read_to_string(&mut toml)
        .unwrap();
    toml::from_str::<Cargo>(&toml).unwrap()
}
