use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cmr_assets::{ReviewedProfile, catalog_standard_source, import_source, verify_source};

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cmr-assets: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl Iterator<Item = OsString>) -> Result<String, String> {
    let mut args = args;
    let command = args.next().ok_or_else(usage)?;
    let options = parse_options(args)?;
    match command.to_str() {
        Some("catalog") => {
            let source = required_path(&options, "--source")?;
            let id = required_text(&options, "--id")?;
            let output = required_path(&options, "--output-profile")?;
            reject_unknown(&options, &["--source", "--id", "--output-profile"])?;
            let profile =
                catalog_standard_source(&source, id).map_err(|error| error.to_string())?;
            let mut document =
                serde_json::to_vec_pretty(&profile).map_err(|error| error.to_string())?;
            document.push(b'\n');
            write_new(&output, &document)?;
            Ok(format!(
                "profile={} files={} output={}",
                profile.id,
                profile.files.len(),
                output.display()
            ))
        }
        Some("verify") => {
            let source = required_path(&options, "--source")?;
            let profile_path = required_path(&options, "--profile")?;
            reject_unknown(&options, &["--source", "--profile"])?;
            let reviewed = load_profile(&profile_path)?;
            let summary =
                verify_source(&source, reviewed.profile()).map_err(|error| error.to_string())?;
            serde_json::to_string(&summary).map_err(|error| error.to_string())
        }
        Some("import") => {
            let source = required_path(&options, "--source")?;
            let profile_path = required_path(&options, "--profile")?;
            let output = required_path(&options, "--output")?;
            reject_unknown(&options, &["--source", "--profile", "--output"])?;
            let reviewed = load_profile(&profile_path)?;
            let summary =
                import_source(&source, &output, &reviewed).map_err(|error| error.to_string())?;
            serde_json::to_string(&summary).map_err(|error| error.to_string())
        }
        _ => Err(usage()),
    }
}

fn parse_options(
    mut args: impl Iterator<Item = OsString>,
) -> Result<BTreeMap<String, OsString>, String> {
    let mut options = BTreeMap::new();
    while let Some(option) = args.next() {
        let option = option
            .into_string()
            .map_err(|_| "option name is not UTF-8".to_owned())?;
        if !option.starts_with("--") {
            return Err(format!("unexpected positional argument {option:?}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {option}"))?;
        if options.insert(option.clone(), value).is_some() {
            return Err(format!("duplicate option {option}"));
        }
    }
    Ok(options)
}

fn required_path(options: &BTreeMap<String, OsString>, name: &str) -> Result<PathBuf, String> {
    options
        .get(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing required option {name}"))
}

fn required_text(options: &BTreeMap<String, OsString>, name: &str) -> Result<String, String> {
    options
        .get(name)
        .ok_or_else(|| format!("missing required option {name}"))?
        .clone()
        .into_string()
        .map_err(|_| format!("{name} is not UTF-8"))
}

fn reject_unknown(options: &BTreeMap<String, OsString>, allowed: &[&str]) -> Result<(), String> {
    if let Some(name) = options
        .keys()
        .find(|name| !allowed.contains(&name.as_str()))
    {
        return Err(format!("unknown option {name}"));
    }
    Ok(())
}

fn load_profile(path: &Path) -> Result<ReviewedProfile, String> {
    let document = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    ReviewedProfile::from_json(&document).map_err(|error| format!("{}: {error}", path.display()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn usage() -> String {
    "usage: cmr-assets <catalog|verify|import> [options]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> impl Iterator<Item = OsString> {
        values.iter().map(OsString::from)
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_options() {
        assert!(run(args(&["verify", "--source", "somewhere"])).is_err());
        assert!(
            run(args(&[
                "verify",
                "--source",
                "a",
                "--source",
                "b",
                "--profile",
                "p"
            ]))
            .unwrap_err()
            .contains("duplicate")
        );
        assert!(
            run(args(&[
                "verify",
                "--source",
                "a",
                "--profile",
                "p",
                "--surprise",
                "x"
            ]))
            .unwrap_err()
            .contains("unknown")
        );
    }

    #[test]
    fn every_command_requires_an_explicit_source_path() {
        for arguments in [
            &[
                "catalog",
                "--id",
                "local",
                "--output-profile",
                "profile.json",
            ][..],
            &["verify", "--profile", "profile.json"],
            &[
                "import",
                "--profile",
                "profile.json",
                "--output",
                "runtime-data",
            ],
        ] {
            assert_eq!(
                run(args(arguments)),
                Err("missing required option --source".to_owned())
            );
        }
    }
}
