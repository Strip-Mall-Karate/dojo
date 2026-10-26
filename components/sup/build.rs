// Inline common build behavior
include!("../libbuild.rs");

use std::{env,
          fs::File,
          io::{self,
               Write},
          path::{Path,
                 PathBuf},
          process::{Command,
                    ExitStatus}};

fn main() {
    dojo::common();
    generate_apidocs();
}

fn generate_apidocs() {
    let dst = Path::new(&env::var("OUT_DIR").unwrap()).join("api.html");
    match env::var("CARGO_FEATURE_APIDOCS") {
        Ok(_) => {
            let src = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("doc/api.raml");
            let cmd = raml2html_cmd(dst, src).expect("failed to compile html from raml");

            assert!(cmd.success());
        }
        Err(_) => {
            let mut file = File::create(dst).unwrap();
            file.write_all(b"No API docs provided at build").unwrap();
        }
    };
}

fn raml2html_cmd(dst: PathBuf, src: PathBuf) -> io::Result<ExitStatus> {
    Command::new("raml2html").arg("-i")
                             .arg(src)
                             .arg("-o")
                             .arg(dst)
                             .status()
}
