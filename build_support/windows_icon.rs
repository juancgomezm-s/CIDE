//! Numeric Win32 .res entries, emitted with std only and linked by existing MSVC.
//! Header/layout references:
//! https://learn.microsoft.com/en-us/windows/win32/menurc/resourceheader
//! https://learn.microsoft.com/en-us/windows/win32/menurc/resource-file-formats
use std::{env, fs, path::PathBuf};

fn word(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}
fn dword(data: &[u8], at: usize) -> usize {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize
}
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8], language: u16) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for value in [0xffff, kind, 0xffff, id] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(if kind == 0 { 0u16 } else { 0x30u16 }).to_le_bytes());
    out.extend_from_slice(&language.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}
pub fn build(icon: &str) {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let path = root.join("assets/branding").join(icon);
    println!("cargo:rerun-if-changed={}", path.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("build_support/windows_icon.rs").display()
    );
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let ico = fs::read(&path).expect("read supplied CIDE icon");
    assert!(
        (6..=4 * 1024 * 1024).contains(&ico.len()),
        "icon file quota"
    );
    assert!(word(&ico, 0) == 0 && word(&ico, 2) == 1, "ICO header");
    let count = word(&ico, 4) as usize;
    assert!(
        (1..=16).contains(&count) && ico.len() >= 6 + 16 * count,
        "ICO frame count"
    );
    let mut resource = Vec::new();
    entry(&mut resource, 0, 0, &[], 0);
    let mut group = Vec::new();
    for value in [0u16, 1, count as u16] {
        group.extend_from_slice(&value.to_le_bytes());
    }
    for index in 0..count {
        let at = 6 + index * 16;
        let length = dword(&ico, at + 8);
        let offset = dword(&ico, at + 12);
        assert!(
            offset >= 6 + 16 * count
                && offset
                    .checked_add(length)
                    .is_some_and(|end| end <= ico.len()),
            "ICO frame bounds"
        );
        let id = index as u16 + 1;
        entry(&mut resource, 3, id, &ico[offset..offset + length], 0x409);
        group.extend_from_slice(&ico[at..at + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    entry(&mut resource, 14, 101, &group, 0x409);
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("cide-icon.res");
    fs::write(&output, resource).expect("write CIDE resource");
    println!("cargo:rustc-link-arg-bins={}", output.display());
}
