// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the MIT license. See LICENSE in the repository root.

//! `doj-plan-build`: loads a Fennel plan. `show` prints its metadata as JSON;
//! `build` builds it for this host and installs it under Dojo's root.

use std::{env,
          path,
          process::ExitCode};

use dojo_plan::{DojPkgPath,
                Located,
                Options,
                Plan,
                TARGETS,
                host_target,
                locate};

const USAGE: &str = "usage: doj-plan-build show <plan-dir> [--target <target>]
       doj-plan-build build <plan-dir> [--target <target>] [--check]";

struct Args {
    command: String,
    dir:     String,
    target:  Option<String>,
    check:   bool,
}

fn parse(args: &[String]) -> Option<Args> {
    let (command, rest) = args.split_first()?;
    if command != "show" && command != "build" {
        return None;
    }
    let mut parsed = Args { command: command.clone(),
                            dir:     String::new(),
                            target:  None,
                            check:   false, };
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--target" if parsed.target.is_none() => parsed.target = Some(rest.next()?.clone()),
            "--check" if command == "build" && !parsed.check => parsed.check = true,
            flag if flag.starts_with("--") => return None,
            dir if parsed.dir.is_empty() => parsed.dir = dir.to_string(),
            _ => return None,
        }
    }
    (!parsed.dir.is_empty()).then_some(parsed)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(args) = parse(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if let Some(target) = &args.target
       && !TARGETS.contains(&target.as_str())
    {
        eprintln!("doj-plan-build: unknown target `{target}`; known targets: {}",
                  TARGETS.join(", "));
        return ExitCode::from(2);
    }

    let located = locate(&args.dir);
    let plan = match Plan::load(&located.plan_dir) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("doj-plan-build: {error}");
            return ExitCode::FAILURE;
        }
    };
    if args.command == "show" {
        show(&plan, args.target.as_deref())
    } else {
        build(&plan, &located, args.target.as_deref(), args.check)
    }
}

fn show(plan: &Plan, target: Option<&str>) -> ExitCode {
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

fn build(plan: &Plan, located: &Located, target: Option<&str>, check: bool) -> ExitCode {
    let Some(host) = host_target() else {
        eprintln!("doj-plan-build: this host is not one of Dojo's targets ({})",
                  TARGETS.join(", "));
        return ExitCode::FAILURE;
    };
    let target = target.unwrap_or(host);
    // Cross-building is not supported: a target runs on this host or not at
    // all. `x86_64-linux-kernel2` packages build on `x86_64-linux` hosts.
    if target != host && !(host == "x86_64-linux" && target == "x86_64-linux-kernel2") {
        eprintln!("doj-plan-build: cannot build `{target}` packages on a `{host}` host");
        return ExitCode::FAILURE;
    }
    let (plan_dir, context) =
        match (path::absolute(&located.plan_dir), path::absolute(&located.context)) {
            (Ok(plan_dir), Ok(context)) => (plan_dir, context),
            (Err(error), _) | (_, Err(error)) => {
                eprintln!("doj-plan-build: cannot resolve the plan directory: {error}");
                return ExitCode::FAILURE;
            }
        };
    let mut options = Options::new(target, &plan_dir, &context);
    options.check = check;
    match plan.build(&options, &DojPkgPath::from_env()) {
        Ok(built) => {
            println!("{}", built.ident);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("doj-plan-build: {error}");
            ExitCode::FAILURE
        }
    }
}
