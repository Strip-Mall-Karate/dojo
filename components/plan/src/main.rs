// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the MIT license. See LICENSE in the repository root.

//! `doj-plan-build`: loads a Fennel plan. `show` prints its metadata as JSON.

use std::{env,
          process::ExitCode};

use dojo_plan::{Plan,
                TARGETS};

const USAGE: &str = "usage: doj-plan-build show <plan-dir> [--target <target>]";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (dir, target) = match args.as_slice() {
        [command, dir] if command == "show" => (dir, None),
        [command, dir, flag, target] if command == "show" && flag == "--target" => {
            (dir, Some(target.as_str()))
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Some(target) = target
       && !TARGETS.contains(&target)
    {
        eprintln!("doj-plan-build: unknown target `{target}`; known targets: {}",
                  TARGETS.join(", "));
        return ExitCode::from(2);
    }

    let plan = match Plan::load(dir) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("doj-plan-build: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(target) = target
       && !plan.metadata().targets().iter().any(|t| t == target)
    {
        eprintln!("doj-plan-build: the plan does not build for `{target}`");
        return ExitCode::FAILURE;
    }
    let json = plan.metadata().to_json(target);
    println!("{}",
             serde_json::to_string_pretty(&json).expect("metadata is plain JSON values"));
    ExitCode::SUCCESS
}
