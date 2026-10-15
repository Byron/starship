use super::env::Env;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub(crate) fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd", test))]
    fn parse(value: &str) -> Option<Self> {
        if let Some(value) = value.strip_prefix('#') {
            if value.len() != 6 || !value.is_ascii() {
                return None;
            }
            return Some(Self(
                u8::from_str_radix(&value[..2], 16).ok()?,
                u8::from_str_radix(&value[2..4], 16).ok()?,
                u8::from_str_radix(&value[4..], 16).ok()?,
            ));
        }
        let mut channels = value.strip_prefix("rgb:")?.split('/');
        let mut channel = || {
            let value = channels.next()?;
            if !(1..=4).contains(&value.len()) || !value.is_ascii() {
                return None;
            }
            let number = u32::from_str_radix(value, 16).ok()?;
            Some((number * 255 / ((1 << (value.len() * 4)) - 1)) as u8)
        };
        let rgb = Self(channel()?, channel()?, channel()?);
        channels.next().is_none().then_some(rgb)
    }

    fn luminance(self) -> f64 {
        let linear = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.0) + 0.7152 * linear(self.1) + 0.0722 * linear(self.2)
    }

    fn contrast(self, other: Self) -> f64 {
        let a = self.luminance();
        let b = other.luminance();
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn blend(self, other: Self, amount: f64) -> Self {
        let channel = |a, b| (f64::from(a) * (1.0 - amount) + f64::from(b) * amount).round() as u8;
        Self(
            channel(self.0, other.0),
            channel(self.1, other.1),
            channel(self.2, other.2),
        )
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct TerminalColors {
    pub(crate) background: Option<Rgb>,
    pub(crate) foreground: Option<Rgb>,
    pub(crate) palette: [Option<Rgb>; 16],
}

impl TerminalColors {
    pub(crate) fn background_color(&self, index: usize) -> Option<Rgb> {
        let background = self.background?;
        let accent = self.palette.get(index).copied().flatten()?;
        // Keep the palette hue, tinting a light theme and shading a dark one.
        Some(background.blend(accent, 0.35))
    }

    pub(crate) fn text_color(&self, background: Rgb) -> Rgb {
        if let Some(foreground) = self.foreground
            && foreground.contrast(background) >= 4.5
        {
            return foreground;
        }
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        if black.contrast(background) >= white.contrast(background) {
            black
        } else {
            white
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    fn complete(&self) -> bool {
        self.background.is_some()
            && self.foreground.is_some()
            && self.palette.iter().all(Option::is_some)
    }

    // True means this is a color reply, even if the terminal reports an unset color.
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd", test))]
    fn reply(&mut self, body: &str) -> bool {
        let mut colors = *self;
        if colors.parse_reply(body) {
            *self = colors;
            true
        } else {
            false
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd", test))]
    fn parse_reply(&mut self, body: &str) -> bool {
        let mut fields = body.split(';');
        match fields.next() {
            Some("21") => {
                let mut recognized = false;
                for field in fields {
                    let Some((key, value)) = field.split_once('=') else {
                        return false;
                    };
                    let slot = match key {
                        "background" => &mut self.background,
                        "foreground" => &mut self.foreground,
                        _ => match key
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| self.palette.get_mut(index))
                        {
                            Some(slot) => slot,
                            None => return false,
                        },
                    };
                    if !value.is_empty() && Rgb::parse(value).is_none() {
                        return false;
                    }
                    if let Some(color) = Rgb::parse(value) {
                        *slot = Some(color);
                    }
                    recognized = true;
                }
                recognized
            }
            Some("10" | "11") => {
                let slot = if body.starts_with("10;") {
                    &mut self.foreground
                } else {
                    &mut self.background
                };
                let Some(color) = fields.next().and_then(Rgb::parse) else {
                    return false;
                };
                if fields.next().is_some() {
                    return false;
                }
                *slot = Some(color);
                true
            }
            Some("4") => {
                let mut recognized = false;
                while let Some(index) = fields.next() {
                    let Some(slot) = index
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| self.palette.get_mut(index))
                    else {
                        return false;
                    };
                    let Some(color) = fields.next().and_then(Rgb::parse) else {
                        return false;
                    };
                    *slot = Some(color);
                    recognized = true;
                }
                recognized
            }
            _ => false,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd", test))]
fn prefers_kitty(env: &Env) -> bool {
    env.get_env("KITTY_WINDOW_ID").is_some()
        || env
            .get_env("TERM")
            .is_some_and(|term| matches!(term.as_str(), "xterm-kitty" | "xterm-ghostty"))
        || env.get_env("TERM_PROGRAM").is_some_and(|term| {
            matches!(
                term.to_ascii_lowercase().as_str(),
                "kitty" | "ghostty" | "rustty"
            )
        })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd", test))]
fn color_query(kitty: bool) -> String {
    if kitty {
        let mut query = String::from("\x1b]21;background=?;foreground=?");
        for index in 0..16 {
            query.push_str(&format!(";{index}=?"));
        }
        query.push_str("\x1b\\");
        query
    } else {
        let mut query = String::from("\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b]4");
        for index in 0..16 {
            query.push_str(&format!(";{index};?"));
        }
        query.push_str("\x1b\\");
        query
    }
}

// Retain partial frames across reads and return every byte that isn't a color reply.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd", test))]
fn extract_replies(pending: &mut Vec<u8>, input: &mut Vec<u8>, colors: &mut TerminalColors) {
    loop {
        let Some(start) = pending.windows(2).position(|bytes| bytes == b"\x1b]") else {
            let keep = usize::from(pending.last() == Some(&0x1b));
            input.extend(pending.drain(..pending.len() - keep));
            return;
        };
        input.extend(pending.drain(..start));
        let end = (2..pending.len()).find_map(|index| {
            if pending[index] == 7 {
                Some((index, 1))
            } else if pending[index..].starts_with(b"\x1b\\") {
                Some((index, 2))
            } else {
                None
            }
        });
        let Some((end, terminator)) = end else { return };
        let recognized = std::str::from_utf8(&pending[2..end]).is_ok_and(|body| colors.reply(body));
        let frame = pending.drain(..end + terminator);
        if recognized {
            drop(frame);
        } else {
            input.extend(frame);
        }
    }
}

pub(crate) fn query(env: &Env) -> Option<TerminalColors> {
    let term = env.get_env("TERM")?;
    if term == "dumb" || term == "linux" {
        return None;
    }
    // shortcut: replay needs kernel support and replies after the deadline reach the shell;
    // use shell-coordinated input to support multiplexers and late replies.
    if env.get_env("TMUX").is_some()
        || env.get_env("STY").is_some()
        || term.starts_with("screen")
        || term.starts_with("tmux")
    {
        return None;
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    {
        tty::query(prefers_kitty(env))
            .map_err(|error| log::debug!("Terminal color query: {error}"))
            .ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
    {
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
mod tty {
    use super::*;
    use nix::libc;
    use nix::sys::select::{FdSet, select};
    use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};
    use nix::sys::time::{TimeVal, TimeValLike};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::fd::{AsFd, AsRawFd};
    use std::time::{Duration, Instant};

    static QUERY_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    struct Terminal {
        file: File,
        original: Termios,
        input: Vec<u8>,
        pending: Vec<u8>,
    }

    impl Terminal {
        fn ready(&self, timeout: u16) -> io::Result<bool> {
            if self.file.as_raw_fd() as usize >= nix::sys::select::FD_SETSIZE {
                return Err(io::Error::other("Terminal descriptor exceeds select limit"));
            }
            let mut fds = FdSet::new();
            fds.insert(self.file.as_fd());
            let mut timeout = TimeVal::milliseconds(i64::from(timeout));
            Ok(select(None, &mut fds, None, None, &mut timeout)? > 0)
        }

        fn receive(&mut self, colors: &mut TerminalColors, deadline: Instant) -> io::Result<()> {
            let mut bytes = [0; 1024];
            while Instant::now() < deadline {
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(100) as u16;
                if !self.ready(remaining)? {
                    break;
                }
                let count = match self.file.read(&mut bytes) {
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                    result => result?,
                };
                if count == 0 {
                    break;
                }
                self.pending.extend_from_slice(&bytes[..count]);
                extract_replies(&mut self.pending, &mut self.input, colors);
                if self.pending.len() + self.input.len() > 16384 {
                    break;
                }
                if colors.complete() && self.pending.is_empty() && !self.ready(0)? {
                    break;
                }
            }
            Ok(())
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            self.input.append(&mut self.pending);
            for byte in &self.input {
                // SAFETY: the ioctl reads one byte from a valid pointer. No input is executed by Starship.
                if unsafe { libc::ioctl(self.file.as_raw_fd(), libc::TIOCSTI, byte as *const u8) }
                    < 0
                {
                    log::warn!(
                        "Could not restore terminal input: {}",
                        io::Error::last_os_error()
                    );
                    break;
                }
            }
            if let Err(error) = tcsetattr(&self.file, SetArg::TCSANOW, &self.original) {
                log::warn!("Could not restore terminal settings: {error}");
            }
        }
    }

    pub(super) fn query(kitty: bool) -> io::Result<TerminalColors> {
        let _lock = QUERY_LOCK
            .try_lock()
            .ok_or_else(|| io::Error::other("Terminal query in progress"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
            .open("/dev/tty")?;
        if nix::unistd::tcgetpgrp(&file)? != nix::unistd::getpgrp() {
            return Err(io::Error::other("Not the foreground process group"));
        }
        let original = tcgetattr(&file)?;
        let deadline = Instant::now() + Duration::from_millis(100);
        let mut terminal = Terminal {
            file,
            original,
            input: Vec::new(),
            pending: Vec::new(),
        };
        let mut raw = terminal.original.clone();
        cfmakeraw(&mut raw);
        tcsetattr(&terminal.file, SetArg::TCSANOW, &raw)?;
        if terminal.ready(0)? {
            return Err(io::Error::other("Terminal input pending"));
        }
        // Verify replay before reading input. Remove the harmless probe without flushing keys.
        let probe = 0u8;
        // SAFETY: TIOCSTI reads one byte from this valid pointer.
        if unsafe {
            libc::ioctl(
                terminal.file.as_raw_fd(),
                libc::TIOCSTI,
                &probe as *const u8,
            )
        } < 0
        {
            return Err(io::Error::other("Terminal input replay unavailable"));
        }
        let probe_deadline = Instant::now() + Duration::from_millis(10);
        loop {
            if Instant::now() >= probe_deadline {
                return Err(io::Error::other("Terminal input probe was not returned"));
            }
            let mut byte = [0];
            match terminal.file.read(&mut byte) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    terminal.ready(1)?;
                    continue;
                }
                Err(error) => return Err(error),
                Ok(0) => return Err(io::Error::other("Terminal closed")),
                Ok(_) => {}
            }
            if byte[0] == probe {
                break;
            }
            terminal.input.push(byte[0]);
        }
        let mut colors = TerminalColors::default();
        terminal.file.write_all(color_query(kitty).as_bytes())?;
        terminal.receive(
            &mut colors,
            if kitty {
                (Instant::now() + Duration::from_millis(40)).min(deadline)
            } else {
                deadline
            },
        )?;
        if kitty && !colors.complete() {
            terminal.file.write_all(color_query(false).as_bytes())?;
            terminal.receive(&mut colors, deadline)?;
        }
        if colors.background.is_none() {
            return Err(io::Error::other("No terminal background reply"));
        }
        Ok(colors)
    }

    use std::os::unix::fs::OpenOptionsExt;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_terminal_rgb_precisions() {
        for value in [
            "rgb:f/8/0",
            "rgb:ff/88/00",
            "rgb:fff/888/000",
            "rgb:ffff/8888/0000",
            "#ff8800",
        ] {
            assert_eq!(Rgb::parse(value), Some(Rgb(255, 136, 0)));
        }
        for value in [
            "rgb:/0/0",
            "rgb:fffff/0/0",
            "rgb:ff/ff/ff/ff",
            "rgb:zz/0/0",
            "#💖ff",
            "#ffffff00",
        ] {
            assert_eq!(Rgb::parse(value), None);
        }
    }

    #[test]
    fn parses_both_protocols_and_preserves_other_input() {
        let mut colors = TerminalColors::default();
        let mut input = Vec::new();
        let mut pending = Vec::new();
        for byte in b"typed\x1b]21;background=rgb:12/34/56;foreground=#ffffff;1=rgb:aa/bb/cc\x1b\\\x1b[A\x1b]4;9;rgb:ff/00/00\x07\x1b]11;rgb:11/22/33\x1b\\\x1b]0;title\x07" {
            pending.push(*byte);
            extract_replies(&mut pending, &mut input, &mut colors);
        }
        assert!(pending.is_empty());
        assert_eq!(input, b"typed\x1b[A\x1b]0;title\x07");
        assert_eq!(colors.background, Some(Rgb(17, 34, 51)));
        assert_eq!(colors.foreground, Some(Rgb(255, 255, 255)));
        assert_eq!(colors.palette[1], Some(Rgb(170, 187, 204)));
        assert_eq!(colors.palette[9], Some(Rgb(255, 0, 0)));
        assert!(!colors.reply("4;256;rgb:ff/ff/ff"));
        assert!(!colors.reply("21;background=garbage"));
        assert!(!colors.reply("21;background=#000000;unknown=#ffffff"));
        assert_eq!(colors.background, Some(Rgb(17, 34, 51)));
    }

    #[test]
    fn derives_backgrounds_and_readable_text_from_actual_theme() {
        for background in [Rgb(15, 20, 30), Rgb(245, 240, 230)] {
            let mut colors = TerminalColors {
                background: Some(background),
                foreground: Some(Rgb(128, 128, 128)),
                ..Default::default()
            };
            colors.palette[1] = Some(Rgb(200, 60, 80));
            let derived = colors.background_color(1).unwrap();
            assert_ne!(derived, background);
            assert_ne!(derived, colors.palette[1].unwrap());
            assert!(colors.text_color(derived).contrast(derived) >= 4.5);
            assert_eq!(colors.background_color(2), None);
        }
    }

    #[test]
    fn detects_kitty_compatible_terminals() {
        for (key, value) in [
            ("TERM", "xterm-kitty"),
            ("TERM", "xterm-ghostty"),
            ("TERM_PROGRAM", "Rustty"),
            ("KITTY_WINDOW_ID", "1"),
        ] {
            let mut env = Env::default();
            env.insert(key, value.to_owned());
            assert!(prefers_kitty(&env));
        }
        assert!(!prefers_kitty(&Env::default()));
        assert!(color_query(true).contains(";15=?"));
        assert!(color_query(false).contains(";15;?"));
    }
}
