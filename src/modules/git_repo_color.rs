use super::{Context, Module, ModuleConfig};

use crate::configs::git_repo_color::GitRepoColorConfig;
use crate::formatter::StringFormatter;

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

    let parsed = StringFormatter::new(config.format).and_then(|formatter| {
        formatter
            .map_meta(|variable, _| match variable {
                "symbol" => Some(config.symbol),
                _ => None,
            })
            .map_style(|variable| match variable {
                "style" => Some(Ok(style)),
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
    use crate::test::{FixtureProvider, ModuleRenderer, fixture_repo};
    use nu_ansi_term::Color;

    const GIT: FixtureProvider = FixtureProvider::Git {
        reftable: false,
        bare: false,
    };

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
