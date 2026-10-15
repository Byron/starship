use super::{Context, Module, ModuleConfig};

use crate::configs::git_repo_color::GitRepoColorConfig;
use crate::formatter::StringFormatter;
use crate::utils::terminal::TerminalColors;
use nu_ansi_term::Color;
use std::borrow::Cow;

pub fn module<'a>(context: &'a Context) -> Option<Module<'a>> {
    let mut module = context.new_module("git_repo_color");
    let config = GitRepoColorConfig::try_load(module.config);

    if config.disabled {
        return None;
    }

    let repo = context.get_git_repo().ok()?;
    let repo_root = repo.workdir.as_ref()?;
    let repo_name = repo_root.file_name()?.to_string_lossy();
    let style = select_style(&repo_name, &config)?;
    let style = if config.adapt_background {
        adapt_background(style, context)
    } else {
        Cow::Borrowed(style)
    };

    let parsed = StringFormatter::new(config.format).and_then(|formatter| {
        formatter
            .map_meta(|variable, _| match variable {
                "symbol" => Some(config.symbol),
                _ => None,
            })
            .map_style(|variable| match variable {
                "style" => Some(Ok(style.as_ref())),
                _ => None,
            })
            .parse(None, Some(context))
    });

    module.set_segments(match parsed {
        Ok(segments) => segments,
        Err(error) => {
            log::warn!("Error in module `git_repo_color`: {error}");
            return None;
        }
    });

    Some(module)
}

fn adapt_background<'a>(style: &'a str, context: &Context) -> Cow<'a, str> {
    let Some(parsed) = crate::config::parse_style_string(style, Some(context)) else {
        return Cow::Borrowed(style);
    };
    let parsed = parsed.to_ansi_style(None);
    let Some(background) = parsed.background.and_then(palette_index) else {
        return Cow::Borrowed(style);
    };
    let Some(colors) = context.terminal_colors() else {
        return Cow::Borrowed(style);
    };
    background_style(style, background, colors, parsed.foreground.is_some())
}

fn palette_index(color: Color) -> Option<usize> {
    Some(match color {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Purple => 5,
        Color::Cyan => 6,
        Color::White => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightPurple => 13,
        Color::LightCyan => 14,
        Color::LightGray => 15,
        Color::Fixed(index) if index < 16 => usize::from(index),
        _ => return None,
    })
}

fn background_style<'a>(
    style: &'a str,
    index: usize,
    colors: &TerminalColors,
    has_foreground: bool,
) -> Cow<'a, str> {
    let Some(background) = colors.background_color(index) else {
        return Cow::Borrowed(style);
    };
    let mut style = format!("{style} bg:{}", background.hex());
    // An explicit foreground remains the user's choice.
    if !has_foreground {
        style.push_str(&format!(" fg:{}", colors.text_color(background).hex()));
    }
    Cow::Owned(style)
}

fn select_style<'a>(repo_name: &str, config: &GitRepoColorConfig<'a>) -> Option<&'a str> {
    config
        .styles
        .get(repo_name)
        .copied()
        .or_else(|| fallback_style(repo_name, &config.fallback_styles))
}

fn fallback_style<'a>(repo_name: &str, fallback_styles: &[&'a str]) -> Option<&'a str> {
    if fallback_styles.is_empty() {
        return None;
    }

    let index = stable_hash(repo_name) as usize % fallback_styles.len();
    fallback_styles.get(index).copied()
}

fn stable_hash(input: &str) -> u64 {
    input.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{FixtureProvider, ModuleRenderer, fixture_repo};
    use crate::utils::terminal::Rgb;
    use nu_ansi_term::Color;

    const GIT: FixtureProvider = FixtureProvider::Git {
        reftable: false,
        bare: false,
    };

    #[test]
    fn default_palette_has_twelve_stable_choices() {
        let config = GitRepoColorConfig::default();
        assert_eq!(config.fallback_styles.len(), 12);
        for style in &config.fallback_styles {
            assert!(crate::config::parse_style_string(style, None).is_some());
        }
        assert_eq!(stable_hash("starship"), 0xf54bee994aeaefeb);
        assert_eq!(
            select_style("starship", &config),
            Some("bold bright-purple")
        );
    }

    #[test]
    fn adapts_background_and_preserves_explicit_foreground() {
        let mut colors = TerminalColors {
            background: Some(Rgb(0, 0, 0)),
            foreground: Some(Rgb(255, 255, 255)),
            ..Default::default()
        };
        colors.palette[1] = Some(Rgb(200, 100, 0));
        assert_eq!(
            background_style("bold bg:red", 1, &colors, false),
            "bold bg:red bg:#462300 fg:#ffffff"
        );
        assert_eq!(
            background_style("fg:my_palette_color bg:red", 1, &colors, true),
            "fg:my_palette_color bg:red bg:#462300"
        );
        assert_eq!(background_style("bg:green", 2, &colors, false), "bg:green");
        assert_eq!(palette_index(Color::Rgb(1, 2, 3)), None);
        assert_eq!(palette_index(Color::Fixed(200)), None);
    }

    #[test]
    fn backgrounds_render_when_query_unavailable_or_disabled() -> std::io::Result<()> {
        let repo_dir = fixture_repo(GIT)?;
        for adapt_background in [true, false] {
            let actual = ModuleRenderer::new("git_repo_color")
                .path(repo_dir.path())
                .config(toml::toml! {
                    [git_repo_color]
                    symbol = " "
                    format = "[$symbol]($style)"
                    fallback_styles = ["bg:red"]
                    adapt_background = adapt_background
                })
                .collect();
            assert_eq!(
                actual,
                Some(
                    nu_ansi_term::Style::new()
                        .on(Color::Red)
                        .paint(" ")
                        .to_string()
                )
            );
        }
        repo_dir.close()
    }

    #[test]
    fn no_output_outside_git_repo() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;

        let actual = ModuleRenderer::new("git_repo_color")
            .path(dir.path())
            .collect();

        assert_eq!(actual, None);
        dir.close()
    }

    #[test]
    fn renders_fallback_style_inside_git_repo() -> std::io::Result<()> {
        let repo_dir = fixture_repo(GIT)?;

        let actual = ModuleRenderer::new("git_repo_color")
            .path(repo_dir.path())
            .config(toml::toml! {
                [git_repo_color]
                fallback_styles = ["bold green"]
            })
            .collect();
        let expected = Some(format!("{} ", Color::Green.bold().paint("■")));

        assert_eq!(actual, expected);
        repo_dir.close()
    }

    #[test]
    fn explicit_style_overrides_fallback_style() -> std::io::Result<()> {
        let repo_dir = fixture_repo(GIT)?;
        let repo_name = repo_dir.path().file_name().unwrap().to_string_lossy();
        let config: toml::Table = toml::from_str(&format!(
            "[git_repo_color]\nfallback_styles = [\"bold red\"]\n[git_repo_color.styles]\n\"{repo_name}\" = \"bold cyan\"\n"
        ))
        .unwrap();

        let actual = ModuleRenderer::new("git_repo_color")
            .path(repo_dir.path())
            .config(config)
            .collect();
        let expected = Some(format!("{} ", Color::Cyan.bold().paint("■")));

        assert_eq!(actual, expected);
        repo_dir.close()
    }

    #[test]
    fn empty_fallback_styles_hides_unmapped_repos() -> std::io::Result<()> {
        let repo_dir = fixture_repo(GIT)?;

        let actual = ModuleRenderer::new("git_repo_color")
            .path(repo_dir.path())
            .config(toml::toml! {
                [git_repo_color]
                fallback_styles = []
            })
            .collect();

        assert_eq!(actual, None);
        repo_dir.close()
    }

    #[test]
    fn custom_symbol_and_format() -> std::io::Result<()> {
        let repo_dir = fixture_repo(GIT)?;

        let actual = ModuleRenderer::new("git_repo_color")
            .path(repo_dir.path())
            .config(toml::toml! {
                [git_repo_color]
                symbol = "x"
                format = "[$symbol]($style)"
                fallback_styles = ["bold green"]
            })
            .collect();
        let expected = Some(format!("{}", Color::Green.bold().paint("x")));

        assert_eq!(actual, expected);
        repo_dir.close()
    }

    #[test]
    fn disabled() -> std::io::Result<()> {
        let repo_dir = fixture_repo(GIT)?;

        let actual = ModuleRenderer::new("git_repo_color")
            .path(repo_dir.path())
            .config(toml::toml! {
                [git_repo_color]
                disabled = true
            })
            .collect();

        assert_eq!(actual, None);
        repo_dir.close()
    }
}
