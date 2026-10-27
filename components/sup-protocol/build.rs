use std::{collections::HashMap,
          env,
          fs,
          io::{Result,
               Write},
          path::{Path,
                 PathBuf}};

use heck::ToSnakeCase;
use prost::Message;
use prost_types::{DescriptorProto,
                  FileDescriptorProto,
                  FileDescriptorSet};

type Module = Vec<String>;

fn main() {
    if env::var("CARGO_FEATURE_PROTOCOLS").is_ok() {
        generate_protocols();
    }
}

fn generate_protocols() {
    let mut config = prost_build::Config::new();
    config.type_attribute(".", "#[derive(Serialize, Deserialize)]");
    config.type_attribute(".", "#[serde(rename_all = \"kebab-case\")]");
    let descriptor_set = PathBuf::from(env::var("OUT_DIR").unwrap()).join("descriptor-set.bin");
    config.file_descriptor_set_path(&descriptor_set);
    config.compile_protos(&protocol_files(), &protocol_includes())
          .expect("protocols");
    compile_proto_impls(&descriptor_set).expect("protocol-impls");
    fs::remove_file(&descriptor_set).unwrap();
    for file in generated_files() {
        fs::rename(&file,
                   format!("src/generated/{}",
                           file.file_name().unwrap().to_string_lossy())).unwrap();
    }
}

fn generated_files() -> Vec<PathBuf> {
    let mut files = vec![];
    for entry in fs::read_dir(env::var("OUT_DIR").unwrap()).unwrap() {
        let file = entry.unwrap();
        if file.file_name().to_str().unwrap().ends_with(".rs") {
            if file.metadata().unwrap().is_file() {
                files.push(file.path());
            }
        }
    }
    files
}

fn protocol_files() -> Vec<String> {
    let mut files = vec![];
    for entry in fs::read_dir("protocols").unwrap() {
        let file = entry.unwrap();
        // skip vim temp files
        if file.file_name().to_str().unwrap().starts_with(".") {
            continue;
        }
        if file.metadata().unwrap().is_file() {
            files.push(file.path().to_string_lossy().into_owned());
        }
    }
    files
}

fn protocol_includes() -> Vec<String> { vec!["protocols".to_string()] }

fn compile_proto_impls(descriptor_set: &Path) -> Result<()> {
    let target = descriptor_set.parent().unwrap();
    let descriptor_set = FileDescriptorSet::decode(fs::read(descriptor_set)?.as_slice())?;

    let modules = generate(descriptor_set.file);
    for (module, content) in modules {
        let mut filename = module.join(".");
        filename.push_str(".impl.rs");
        let mut file = fs::File::create(target.join(filename))?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
    }

    Ok(())
}

fn generate(files: Vec<FileDescriptorProto>) -> HashMap<Module, String> {
    let mut modules = HashMap::new();
    for file in files {
        let module = module(&file);
        let mut buf = modules.entry(module).or_insert_with(String::new);
        buf.push_str("use crate::message;\n\n");
        for msg in file.message_type.iter() {
            add_message(msg, &mut buf);
        }
    }
    modules
}

pub fn module(file: &FileDescriptorProto) -> Module {
    file.package()
        .split('.')
        .filter(|s| !s.is_empty())
        .map(to_snake)
        .collect()
}

fn add_message(msg: &DescriptorProto, buf: &mut String) {
    buf.push_str(&format!("impl message::MessageStatic for {} {{\n", msg.name()));
    buf.push_str(&format!("    const MESSAGE_ID: &'static str = \"{}\";\n", msg.name()));
    buf.push_str("}\n");
}

/// Converts a `camelCase` or `SCREAMING_SNAKE_CASE` identifier to a `lower_snake` case Rust field
/// identifier.
pub fn to_snake(s: &str) -> String {
    let mut ident = s.to_snake_case();

    // Add a trailing underscore if the identifier matches a Rust keyword
    // (https://doc.rust-lang.org/grammar.html#keywords).
    match &ident[..] {
        "abstract" | "alignof" | "as" | "become" | "box" | "break" | "const" | "continue"
        | "crate" | "do" | "else" | "enum" | "extern" | "false" | "final" | "fn" | "for" | "if"
        | "impl" | "in" | "let" | "loop" | "macro" | "match" | "mod" | "move" | "mut"
        | "offsetof" | "override" | "priv" | "proc" | "pub" | "pure" | "ref" | "return"
        | "self" | "sizeof" | "static" | "struct" | "super" | "trait" | "true" | "type"
        | "typeof" | "unsafe" | "unsized" | "use" | "virtual" | "where" | "while" | "yield" => {
            ident.push('_');
        }
        _ => (),
    }
    ident
}
