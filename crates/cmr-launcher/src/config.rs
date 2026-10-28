//! Command line and the remembered settings file.
//!
//! The settings file is plain `key=value` lines; unknown keys are ignored so
//! older launchers can read newer files.

use std::path::PathBuf;

use crate::registry::LANGUAGES;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub game: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
    pub language: Option<String>,
}

pub const USAGE: &str = "\
CMRLauncher [--game <folder>] [--runtime <folder>] [--language <name>]

  --game      the folder with your installed Rally.exe (remembered)
  --runtime   where the launcher keeps its private copy (remembered)
  --language  ENGLISH, FRENCH, GERMAN, SPANISH or ITALIAN (first run only)";

impl Settings {
    pub fn parse_file(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.trim() {
                "game" => s.game = Some(value.into()),
                "runtime" => s.runtime = Some(value.into()),
                "language" => s.language = Some(value.into()),
                _ => {}
            }
        }
        s
    }

    pub fn to_file(&self) -> String {
        let mut out = String::new();
        let mut put = |k: &str, v: Option<String>| {
            if let Some(v) = v {
                out.push_str(&format!("{k}={v}\n"));
            }
        };
        put("game", self.game.as_ref().map(|p| p.display().to_string()));
        put(
            "runtime",
            self.runtime.as_ref().map(|p| p.display().to_string()),
        );
        put("language", self.language.clone());
        out
    }

    /// Command-line values override remembered ones.
    pub fn merged(self, over: Settings) -> Settings {
        Settings {
            game: over.game.or(self.game),
            runtime: over.runtime.or(self.runtime),
            language: over.language.or(self.language),
        }
    }

    pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Settings, String> {
        let mut s = Settings::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .ok_or_else(|| format!("{name} needs a value\n\n{USAGE}"))
            };
            match arg.as_str() {
                "--game" => s.game = Some(value("--game")?.into()),
                "--runtime" => s.runtime = Some(value("--runtime")?.into()),
                "--language" => {
                    let v = value("--language")?.to_ascii_uppercase();
                    if !LANGUAGES.contains(&v.as_str()) {
                        return Err(format!("unknown language {v}\n\n{USAGE}"));
                    }
                    s.language = Some(v);
                }
                "--help" | "-h" | "/?" => return Err(USAGE.into()),
                other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
            }
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Result<Settings, String> {
        Settings::parse_args(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_arguments() {
        let s = args(&["--game", r"D:\Games\CMR", "--language", "german"]).unwrap();
        assert_eq!(s.game, Some(PathBuf::from(r"D:\Games\CMR")));
        assert_eq!(s.language.as_deref(), Some("GERMAN"));
        assert!(args(&["--language", "klingon"]).is_err());
        assert!(args(&["--game"]).is_err());
        assert!(args(&["--bogus"]).is_err());
    }

    #[test]
    fn file_round_trips_and_args_win() {
        let saved = Settings {
            game: Some(r"C:\old".into()),
            runtime: Some(r"C:\rt".into()),
            language: None,
        };
        assert_eq!(Settings::parse_file(&saved.to_file()), saved);
        let m = saved.merged(args(&["--game", r"C:\new"]).unwrap());
        assert_eq!(m.game, Some(PathBuf::from(r"C:\new")));
        assert_eq!(m.runtime, Some(PathBuf::from(r"C:\rt")));
    }

    #[test]
    fn file_ignores_junk() {
        let s = Settings::parse_file("# comment\nfuture=1\ngame = C:\\G \n\nruntime=\n");
        assert_eq!(s.game, Some(PathBuf::from(r"C:\G")));
        assert_eq!(s.runtime, None);
    }
}
