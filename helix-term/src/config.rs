use crate::keymap;
use crate::keymap::{merge_keys, KeyTrie};
use helix_loader::merge_toml_values;
use helix_view::document::Mode;
use helix_view::editor::ModeConfig;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::Error as IOError;
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

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: None,
            keys: keymap::default_evil(),
            editor: helix_view::editor::Config::default_evil(),
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
    pub fn load(
        global: Result<String, ConfigLoadError>,
        local: Result<String, ConfigLoadError>,
    ) -> Result<Config, ConfigLoadError> {
        let global_config: Result<ConfigRaw, ConfigLoadError> =
            global.and_then(|file| toml::from_str(&file).map_err(ConfigLoadError::BadConfig));
        let local_config: Result<ConfigRaw, ConfigLoadError> =
            local.and_then(|file| toml::from_str(&file).map_err(ConfigLoadError::BadConfig));
        let evil = Self::is_evil(&global_config, &local_config);
        let mut res = match (global_config, local_config) {
            (Ok(global), Ok(local)) => {
                let mut keys = if !evil {
                    keymap::default()
                } else {
                    keymap::default_evil()
                };

                if let Some(global_keys) = global.keys {
                    merge_keys(&mut keys, global_keys)
                }
                if let Some(local_keys) = local.keys {
                    merge_keys(&mut keys, local_keys)
                }

                let editor = match (global.editor, local.editor) {
                    (None, None) => helix_view::editor::Config::default_evil(),
                    (None, Some(val)) | (Some(val), None) => {
                        val.try_into().map_err(ConfigLoadError::BadConfig)?
                    }
                    (Some(global), Some(local)) => merge_toml_values(global, local, 3)
                        .try_into()
                        .map_err(ConfigLoadError::BadConfig)?,
                };

                Config {
                    theme: local.theme.or(global.theme),
                    keys,
                    editor,
                }
            }
            // if any configs are invalid return that first
            (_, Err(ConfigLoadError::BadConfig(err)))
            | (Err(ConfigLoadError::BadConfig(err)), _) => {
                return Err(ConfigLoadError::BadConfig(err))
            }
            (Ok(config), Err(_)) | (Err(_), Ok(config)) => {
                let mut keys = if !evil {
                    keymap::default()
                } else {
                    keymap::default_evil()
                };

                if let Some(keymap) = config.keys {
                    merge_keys(&mut keys, keymap);
                }
                Config {
                    theme: config.theme,
                    keys,
                    editor: config.editor.map_or_else(
                        || Ok(helix_view::editor::Config::default_evil()),
                        |val| val.try_into().map_err(ConfigLoadError::BadConfig),
                    )?,
                }
            }

            // these are just two io errors return the one for the global config
            (Err(err), Err(_)) => return Err(err),
        };

        // HACK: because we can't easily differentiate between "no configuration" and
        // "explicit non-evil mode configuration"
        if evil {
            res.editor.statusline.mode = ModeConfig::default_evil();
        }

        Ok(res)
    }

    fn is_evil(
        global_config: &Result<ConfigRaw, ConfigLoadError>,
        local_config: &Result<ConfigRaw, ConfigLoadError>,
    ) -> bool {
        if local_config.is_ok()
            && local_config.as_ref().unwrap().editor.is_some()
            && local_config
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .get("evil")
                .is_some()
        {
            log::info!("Retrieving evil mode from local config");
            return local_config
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .get("evil")
                .unwrap()
                .as_bool()
                .expect("Incorrect type for `editor.config`, expected `bool`");
        }

        if global_config.is_ok()
            && global_config.as_ref().unwrap().editor.is_some()
            && global_config
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .get("evil")
                .is_some()
        {
            log::info!("Retrieving evil mode from global config");
            return global_config
                .as_ref()
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .get("evil")
                .unwrap()
                .as_bool()
                .expect("Incorrect type for `editor.config`, expected `bool`");
        }

        log::debug!("Evil mode not explicitly set in local/global config, will enable default");
        true
    }

    pub fn load_default() -> Result<Config, ConfigLoadError> {
        let global_config =
            fs::read_to_string(helix_loader::config_file()).map_err(ConfigLoadError::Error);
        let local_config = fs::read_to_string(helix_loader::workspace_config_file())
            .map_err(ConfigLoadError::Error);
        Config::load(global_config, local_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Config {
        fn load_test(config: &str) -> Config {
            Config::load(Ok(config.to_owned()), Err(ConfigLoadError::default())).unwrap()
        }
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

        let mut keys = keymap::default_evil();
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
            Config::load_test(sample_keymaps),
            Config {
                keys,
                ..Default::default()
            }
        );
    }

    #[test]
    fn keys_resolve_to_correct_defaults() {
        // From serde default
        let default_keys = Config::load_test("").keys;
        assert_eq!(default_keys, keymap::default_evil());

        // From the Default trait
        let default_keys = Config::default().keys;
        assert_eq!(default_keys, keymap::default_evil());
    }
}
