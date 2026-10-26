//! UI translations.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LangChoice {
    #[default]
    System,
    Fr,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

const FR_SRC: &str = include_str!("../locales/fr.toml");
const EN_SRC: &str = include_str!("../locales/en.toml");

type Table = HashMap<String, &'static str>;

fn parse(src: &'static str) -> Table {
    let table: toml::Table = toml::from_str(src).expect("invalid locale file");
    // Leaked once at start-up so lookups can hand out `&'static str`.
    table
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, &*Box::leak(s.to_string().into_boxed_str()))))
        .collect()
}

static FR: LazyLock<Table> = LazyLock::new(|| parse(FR_SRC));
static EN: LazyLock<Table> = LazyLock::new(|| parse(EN_SRC));

pub fn resolve(choice: LangChoice) -> Lang {
    match choice {
        LangChoice::Fr => Lang::Fr,
        LangChoice::En => Lang::En,
        LangChoice::System => match sys_locale::get_locale() {
            Some(l) if l.to_lowercase().starts_with("fr") => Lang::Fr,
            _ => Lang::En,
        },
    }
}

/// Translated string for `key`; the key itself when missing.
pub fn tr<'a>(lang: Lang, key: &'a str) -> &'a str {
    let table = match lang {
        Lang::Fr => &*FR,
        Lang::En => &*EN,
    };
    table.get(key).copied().unwrap_or(key)
}

/// `tr` with `{name}` placeholders replaced.
pub fn trf(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = tr(lang, key).to_string();
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(src: &str) -> Vec<String> {
        let t: toml::Table = toml::from_str(src).unwrap();
        let mut k: Vec<String> = t.keys().cloned().collect();
        k.sort();
        k
    }

    #[test]
    fn fr_and_en_have_same_keys() {
        assert_eq!(keys(FR_SRC), keys(EN_SRC));
    }

    #[test]
    fn no_empty_values() {
        for src in [FR_SRC, EN_SRC] {
            let t: toml::Table = toml::from_str(src).unwrap();
            for (k, v) in t {
                assert!(v.as_str().is_some_and(|s| !s.trim().is_empty()), "{k}");
            }
        }
    }

    #[test]
    fn tr_known_and_missing_keys() {
        assert_eq!(tr(Lang::Fr, "repos.add"), "Ajouter un dossier");
        assert_eq!(tr(Lang::En, "repos.add"), "Add a folder");
        assert_eq!(tr(Lang::Fr, "nope.key"), "nope.key");
    }

    #[test]
    fn trf_replaces_placeholders() {
        assert_eq!(trf(Lang::Fr, "time.minutes", &[("n", "5")]), "il y a 5 min");
        assert_eq!(trf(Lang::En, "time.minutes", &[("n", "5")]), "5 min ago");
    }

    #[test]
    fn explicit_choice_wins() {
        assert_eq!(resolve(LangChoice::Fr), Lang::Fr);
        assert_eq!(resolve(LangChoice::En), Lang::En);
    }

    #[test]
    fn own_error_keys_exist() {
        for k in ["error.push_detached", "error.no_remote"] {
            assert_ne!(tr(Lang::Fr, k), k);
        }
    }
}
