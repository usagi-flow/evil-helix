use crate::keymap;
use crate::keymap::{merge_keys, KeyTrie};
use helix_loader::{merge_toml_values_with_strategy, MergeMode, MergeStrategy};
use helix_view::document::Mode;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::Error as IOError;
use std::path::PathBuf;
use toml::de::Error as TomlError;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: Option<String>,
    pub keys: HashMap<Mode, KeyTrie>,
    pub editor: helix_view::editor::Config,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRaw {
    pub theme: Option<String>,
    pub keys: Option<HashMap<Mode, KeyTrie>>,
    pub editor: Option<toml::Value>,
}

impl ConfigRaw {
    pub fn load(path: PathBuf) -> Result<Option<ConfigRaw>, ConfigLoadError> {
        match fs::read_to_string(path) {
            // Don't treat a missing config file as an error.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ConfigLoadError::Error(e)),
            Ok(s) => toml::from_str(&s)
                .map(Some)
                .map_err(ConfigLoadError::BadConfig),
        }
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: None,
            keys: keymap::default(),
            editor: helix_view::editor::Config::default(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigLoadError {
    BadConfig(TomlError),
    Error(IOError),
}

impl Default for ConfigLoadError {
    fn default() -> Self {
        ConfigLoadError::Error(IOError::new(std::io::ErrorKind::NotFound, "place holder"))
    }
}

impl Display for ConfigLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadError::BadConfig(err) => err.fmt(f),
            ConfigLoadError::Error(err) => err.fmt(f),
        }
    }
}

impl Config {
    /// Merge a ConfigRaw value into a Config.
    pub fn apply(&mut self, opt_config_raw: Option<ConfigRaw>) -> Result<(), ConfigLoadError> {
        if let Some(config_raw) = opt_config_raw {
            if let Some(theme) = config_raw.theme {
                self.theme = Some(theme)
            }
            if let Some(keymap) = config_raw.keys {
                merge_keys(&mut self.keys, keymap)
            }
            if let Some(editor) = config_raw.editor {
                // We only know how to merge toml values, so convert back to toml first.
                let val = toml::Value::try_from(&self.editor).unwrap();
                self.editor = merge_toml_values_with_strategy(
                    val,
                    editor,
                    &MergeStrategy {
                        array: MergeMode::Never,
                        table: MergeMode::Always,
                    },
                )
                .try_into()
                .map_err(ConfigLoadError::BadConfig)?
            }
        }
        Ok(())
    }

    pub fn load_default() -> Result<Config, ConfigLoadError> {
        let mut config = Config::default();
        let global = ConfigRaw::load(helix_loader::config_file())?;
        let local = ConfigRaw::load(helix_loader::workspace_config_file())?;
        config.apply(global)?;
        config.apply(local)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Config {
        fn load_test(global: &str, local: &str) -> Config {
            let mut config = Config::default();
            let global = Some(toml::from_str(&global).unwrap());
            let local = Some(toml::from_str(&local).unwrap());
            config.apply(global).unwrap();
            config.apply(local).unwrap();
            config
        }
    }

    #[test]
    fn should_merge_editor_config_tables() {
        let global = r#"
            [editor.statusline]
            mode.insert = "INSERT"
            mode.select = "SELECT"
        "#;
        let local = r#"
            [editor.statusline]
            mode.select = "VIS"
        "#;
        let config = Config::load_test(global, local);
        assert_eq!(config.editor.statusline.mode.normal, "NOR"); // Default
        assert_eq!(config.editor.statusline.mode.insert, "INSERT"); // Global
        assert_eq!(config.editor.statusline.mode.select, "VIS"); // Local
    }

    #[test]
    fn should_override_editor_config_arrays() {
        let global = r#"
            [editor]
            shell = ["bash", "-c"]
        "#;
        let local = r#"
            [editor]
            shell = ["fish", "-c"]
        "#;
        let config = Config::load_test(global, local);
        assert_eq!(config.editor.shell, ["fish", "-c"]);
    }

    #[test]
    fn load_non_existing_config() {
        let path = PathBuf::from(r"does-not-exist");
        let result = ConfigRaw::load(path);
        assert!(result.is_ok_and(|x| x.is_none()));
    }

    #[test]
    fn parsing_keymaps_config_file() {
        use crate::keymap;
        use helix_core::hashmap;
        use helix_view::document::Mode;

        let sample_keymaps = r#"
            [keys.insert]
            y = "move_line_down"
            S-C-a = "delete_selection"

            [keys.normal]
            A-F12 = "move_next_word_end"
        "#;

        let mut keys = keymap::default();
        merge_keys(
            &mut keys,
            hashmap! {
                Mode::Insert => keymap!({ "Insert mode"
                    "y" => move_line_down,
                    "S-C-a" => delete_selection,
                }),
                Mode::Normal => keymap!({ "Normal mode"
                    "A-F12" => move_next_word_end,
                }),
            },
        );

        assert_eq!(
            Config::load_test(sample_keymaps, ""),
            Config {
                keys,
                ..Default::default()
            }
        );
    }

    #[test]
    fn keys_resolve_to_correct_defaults() {
        // From serde default
        let default_keys = Config::load_test("", "").keys;
        assert_eq!(default_keys, keymap::default());

        // From the Default trait
        let default_keys = Config::default().keys;
        assert_eq!(default_keys, keymap::default());
    }
}
