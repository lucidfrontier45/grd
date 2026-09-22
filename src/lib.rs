pub mod asset;
pub mod cli;
pub mod config;
pub mod download;
pub mod extract;
pub mod github;
pub mod state;

use std::io::{BufRead, Write};

/// Prompt the user to confirm an upgrade. Returns `true` only on `y`/`Y`.
pub fn confirm_upgrade(cached_tag: &str, target_tag: &str) -> bool {
    confirm_upgrade_impl(cached_tag, target_tag, &mut std::io::stdin().lock())
}

pub(crate) fn confirm_upgrade_impl(
    cached_tag: &str,
    target_tag: &str,
    input: &mut impl BufRead,
) -> bool {
    print!("Upgrade from {cached_tag} to {target_tag}? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    input.read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y")
}

#[cfg(test)]
mod tests;
