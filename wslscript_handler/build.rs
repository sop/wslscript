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
    println!("cargo:rerun-if-changed=../wslscript/Cargo.toml");
    let handler_cargo = handler_cargo();
    let wslscript_cargo = wslscript_cargo();
    let manifest_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("manifest.xml");
    let mut f = File::create(manifest_path.clone()).unwrap();
    f.write_all(get_manifest(&handler_cargo, &wslscript_cargo).as_bytes())
        .unwrap();
    let now = chrono::Local::now();
    let version = parse_version(&wslscript_cargo.package.version);
    winres::WindowsResource::new()
        .set_manifest_file(manifest_path.to_str().unwrap())
        .set("ProductName", "WSL Script")
        .set("FileDescription", &handler_cargo.package.description)
        .set("FileVersion", &wslscript_cargo.package.version)
        .set_version_info(VersionInfo::FILEVERSION, version)
        .set("ProductVersion", &wslscript_cargo.package.version)
        .set_version_info(VersionInfo::PRODUCTVERSION, version)
        .set(
            "InternalName",
            &format!("{}.dll", handler_cargo.package.name),
        )
        .set(
            "LegalCopyright",
            &format!("Joni Kollani © {}", now.format("%Y")),
        )
        .compile()
        .unwrap();
}

/// Parse version string to resource version.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/menurc/versioninfo-resource
fn parse_version(s: &str) -> u64 {
    // take first 3 numbers
    let mut parts = s
        .split(".")
        .filter_map(|s| {
            s.chars()
                .take_while(|c| c.is_digit(10))
                .collect::<String>()
                .parse::<u16>()
                .ok()
        })
        .take(3)
        .collect::<Vec<_>>();
    // insert 0 as a fourth component
    parts.push(0);
    assert!(parts.len() == 4);
    (parts[0] as u64) << 48 | (parts[1] as u64) << 32 | (parts[2] as u64) << 16 | (parts[3] as u64)
}

/// Format resource version to _m.n.o.p_ string.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/sbscs/assembly-versions
fn format_version(v: u64) -> String {
    format!(
        "{}.{}.{}.{}",
        (v >> 48) & 0xffff,
        (v >> 32) & 0xffff,
        (v >> 16) & 0xffff,
        v & 0xffff
    )
}

fn get_manifest(handler_cargo: &Cargo, wslscript_cargo: &Cargo) -> String {
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
        name = format!("github.sop.{}", handler_cargo.package.name),
        description = handler_cargo.package.description,
        version = format_version(parse_version(&wslscript_cargo.package.version))
    )
}

fn handler_cargo() -> Cargo {
    let mut toml = String::new();
    File::open(PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml"))
        .unwrap()
        .read_to_string(&mut toml)
        .unwrap();
    toml::from_str::<Cargo>(&toml).unwrap()
}

fn wslscript_cargo() -> Cargo {
    let mut toml = String::new();
    File::open(
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
            .parent()
            .unwrap()
            .join("wslscript/Cargo.toml"),
    )
    .unwrap()
    .read_to_string(&mut toml)
    .unwrap();
    toml::from_str::<Cargo>(&toml).unwrap()
}
