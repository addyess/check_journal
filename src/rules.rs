use anyhow::{ensure, Context, Result};
use regex::bytes::RegexSet;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RuleSet {
    matches: RegexSet,
    except: RegexSet,
}

impl RuleSet {
    pub fn new(patterns: &[String], exceptions: &[String], title: &str) -> Result<Self> {
        Ok(Self {
            matches: RegexSet::new(patterns)
                .with_context(|| format!("Failed to load {} patterns", title))?,
            except: RegexSet::new(exceptions)
                .with_context(|| format!("Failed to load {} exceptions", title))?,
        })
    }

    pub fn is_match(&self, line: &[u8]) -> bool {
        self.matches.is_match(line) && !self.except.is_match(line)
    }
}

impl Default for RuleSet {
    fn default() -> Self {
        let empty: [&str; 0] = [];
        Self {
            matches: RegexSet::new(&empty).unwrap(),
            except: RegexSet::new(&empty).unwrap(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RulesFile {
    criticalpatterns: Vec<String>,
    criticalexceptions: Vec<String>,
    warningpatterns: Vec<String>,
    warningexceptions: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Rules {
    pub crit: RuleSet,
    pub warn: RuleSet,
}

impl Rules {
    fn new(source: RulesFile) -> Result<Self> {
        Ok(Self {
            crit: RuleSet::new(
                &source.criticalpatterns,
                &source.criticalexceptions,
                "critical",
            )?,
            warn: RuleSet::new(
                &source.warningpatterns,
                &source.warningexceptions,
                "warning",
            )?,
        })
    }

    fn parse<R: Read>(rdr: R) -> Result<Self> {
        let rulesfile = serde_yaml::from_reader(rdr)?;
        Self::new(rulesfile)
    }

    pub fn load<P: AsRef<Path>>(source: P) -> Result<Self> {
        let source = source.as_ref();
        let s = source.to_string_lossy();
        if s.contains("://") {
            let res = ureq::get(&*s)
                .timeout_connect(30_000)
                .timeout_read(300_000)
                .call();
            ensure!(
                res.ok(),
                "Failed to retrieve remote rules from {}: {}",
                s,
                res.status_line()
            );
            Self::parse(res.into_reader())
        } else {
            Self::parse(
                File::open(&source)
                    .with_context(|| format!("Cannot open rules file {:?}", source))?,
            )
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn load_rules() -> Rules {
        Rules::load(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/rules.yaml"))
            .expect("load fixtures/rules.yaml")
    }

    #[test]
    fn parse_failure_should_be_reported() {
        if let Err(e) = RuleSet::new(
            &["foo".to_owned(), "invalid (re".to_owned(), "bar".to_owned()][..],
            &[],
            "crit",
        ) {
            assert_eq!(format!("{}", e), "Failed to load crit patterns");
        } else {
            panic!("compile() did not return error");
        }
    }

    #[test]
    fn load_from_file() {
        let r = load_rules();
        assert_eq!(r.crit.matches.len(), 2);
        assert_eq!(r.crit.except.len(), 2);
        assert_eq!(r.warn.matches.len(), 2);
        assert_eq!(r.warn.except.len(), 3);
    }

    #[test]
    fn load_from_nonexistent_url_should_fail() {
        assert!(Rules::load("http://no.such.host.example.com/rules").is_err());
    }

    #[test]
    fn matches_and_exceptions() {
        let r = load_rules();
        assert!(r.crit.is_match(b"0 Errors"));
        assert!(!r.crit.is_match(b"0 errors"));
        assert!(r.warn.is_match(b"some WARN foo"));
        assert!(!r.warn.is_match(b"WARN: node[1234]: Exception in function"))
    }
}
