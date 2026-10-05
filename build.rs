// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Embeds the Windows version resource (ProductName = ActiveMQRust).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u16> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap_or(0))
        .collect();
    parts.resize(4, 0);
    let numeric = format!("{},{},{},{}", parts[0], parts[1], parts[2], parts[3]);

    let rc = format!(
        r#"#include <winver.h>
VS_VERSION_INFO VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Matteo Baccan"
      VALUE "FileDescription", "ActiveMQRust message broker"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "mqrust"
      VALUE "LegalCopyright", "Copyright (c) Matteo Baccan. MIT License."
      VALUE "OriginalFilename", "mqrust.exe"
      VALUE "ProductName", "ActiveMQRust"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let rc_path = std::path::Path::new(&out_dir).join("mqrust.rc");
    std::fs::write(&rc_path, rc).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE);
}
