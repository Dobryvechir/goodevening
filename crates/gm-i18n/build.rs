//! Generates `MsgId` and the embedded language table list from `assets/lang`.
use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let lang_dir = manifest.join("../../assets/lang").canonicalize().expect("assets/lang missing");
    println!("cargo:rerun-if-changed={}", lang_dir.display());
    let ids = fs::read_to_string(lang_dir.join("messages.ids")).expect("messages.ids");
    let list = fs::read_to_string(lang_dir.join("languages.lst")).expect("languages.lst");

    let mut out = String::from("/// Message identifiers, generated from `assets/lang/messages.ids`.\n");
    out.push_str("#[allow(non_camel_case_types, clippy::enum_variant_names)]\n");
    out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n#[repr(u32)]\npub enum MsgId {\n");
    let mut n = 0;
    for id in ids.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let _ = writeln!(out, "    {id},");
        n += 1;
    }
    out.push_str("}\n");
    let _ = writeln!(out, "pub const MSG_COUNT: usize = {n};");
    out.push_str("pub(crate) static LANGS: &[(&str, &str, &str)] = &[\n");
    for line in list.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let mut it = line.splitn(3, ' ');
        let file = it.next().unwrap();
        let name = it.next().unwrap();
        let descr = it.next().unwrap().trim_matches('"');
        let path = lang_dir.join(file);
        let _ = writeln!(out, "    ({name:?}, {descr:?}, include_str!({:?})),", path.display().to_string());
    }
    out.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("msg.rs"), out).unwrap();
}
