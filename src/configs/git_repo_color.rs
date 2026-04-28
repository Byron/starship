use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[cfg_attr(
    feature = "config-schema",
    derive(schemars::JsonSchema),
    schemars(deny_unknown_fields)
)]
#[serde(default)]
pub struct GitRepoColorConfig<'a> {
    pub format: &'a str,
    pub symbol: &'a str,
    pub styles: IndexMap<String, &'a str>,
    pub fallback_styles: Vec<&'a str>,
    pub disabled: bool,
}

impl Default for GitRepoColorConfig<'_> {
    fn default() -> Self {
        Self {
            format: "[$symbol]($style) ",
            symbol: "■",
            styles: IndexMap::new(),
            fallback_styles: vec![
                "bold red",
                "bold yellow",
                "bold green",
                "bold cyan",
                "bold blue",
                "bold purple",
            ],
            disabled: false,
        }
    }
}
