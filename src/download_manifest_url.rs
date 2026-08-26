//! Auto-resolves `bera-reth download`'s `--manifest-url` for Berachain chains,
//! so operators do not need to look up or hand-copy a manifest URL for a
//! routine restore.
//!
//! **Binary-only.** This module is declared and rooted in `main.rs`, not
//! `pub mod`-ed from `lib.rs` — it is CLI-argv glue for the `download`
//! subcommand, not part of what the `bera_reth` library exposes.
//!
//! Berachain's chain IDs (mainnet 80094, bepolia 80069) never match Ethereum
//! mainnet, so upstream reth's own snapshot auto-discovery refuses to run at
//! all for them (see `reth_cli_commands::download`'s `mainnet_only_discovery`
//! gate). This module is a thin, argv-level substitute: it constructs the
//! manifest URL from the chain and injects it as if the operator had typed
//! `--manifest-url` themselves. It never touches reth's internal
//! `DownloadCommand` state or requires a fork of reth to work.
//!
//! The manifest object key is fixed per chain (`v2/<chain>/reth/manifest.json`)
//! and is overwritten in place on every publish, not versioned by block or
//! date, so the URL can be constructed directly with no network round trip:
//! `snapshot-v2-publish.sh` always writes to `S3_EL_PREFIX/manifest.json`
//! where `S3_EL_PREFIX = "v2/<chain>/reth"`. This mirrors the object layout
//! documented in `infra-snapshots/project/reference/storage-v2.md`.

const PUBLIC_BASE: &str = "https://bera-snapshots.fsn1.your-objectstorage.com";

/// Chains this resolver knows how to look up. Mirrors `chainspec::mod.rs`'s
/// own `BERACHAIN_MAINNET`/`BERACHAIN_BEPOLIA` chain-name strings, but
/// duplicated rather than imported: those live in the library crate, this
/// module lives in the binary crate, and reusing them would mean exposing
/// them as public library API just to de-duplicate two literals in one
/// bin-only file. Any other `--chain` value (a custom genesis file path,
/// for example) is left untouched.
const KNOWN_CHAINS: &[&str] = &["mainnet", "bepolia"];

/// Rewrites `argv` to inject `--manifest-url` when the caller ran
/// `download --chain <mainnet|bepolia>` without supplying a manifest source
/// of their own. Returns `argv` unchanged for every other invocation:
/// non-`download` commands, a download that already names a manifest
/// source, an unrecognized `--chain` value, or `--list`/`--help`.
pub(crate) fn with_resolved_manifest_url(argv: Vec<String>) -> Vec<String> {
    if !is_bare_download_needing_manifest(&argv) {
        return argv;
    }

    let Some(chain) = chain_arg(&argv) else {
        return argv;
    };

    if !KNOWN_CHAINS.contains(&chain.as_str()) {
        return argv;
    }

    let mut argv = argv;
    argv.push("--manifest-url".to_string());
    argv.push(manifest_url(&chain));
    argv
}

/// The fixed, in-place-overwritten manifest URL for a known chain.
fn manifest_url(chain: &str) -> String {
    format!("{PUBLIC_BASE}/v2/{chain}/reth/manifest.json")
}

/// Returns `true` only for a `download` invocation that has no manifest
/// source of its own and isn't `--list`/`--help` (which don't need one).
fn is_bare_download_needing_manifest(argv: &[String]) -> bool {
    if !argv.iter().any(|a| a == "download") {
        return false;
    }

    let has_explicit_source = argv.iter().any(|a| {
        a == "--manifest-url"
            || a.starts_with("--manifest-url=")
            || a == "--manifest-path"
            || a.starts_with("--manifest-path=")
            || a == "-u"
            || a == "--url"
            || a.starts_with("--url=")
    });
    let is_list_or_help = argv
        .iter()
        .any(|a| a == "--list" || a == "--list-snapshots" || a == "-h" || a == "--help");

    !has_explicit_source && !is_list_or_help
}

/// Extracts the value of `--chain <value>` or `--chain=<value>` from argv.
fn chain_arg(argv: &[String]) -> Option<String> {
    for (i, arg) in argv.iter().enumerate() {
        if let Some(value) = arg.strip_prefix("--chain=") {
            return Some(value.to_string());
        }
        if arg == "--chain" {
            return argv.get(i + 1).cloned();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_known_chain_manifest_urls() {
        assert_eq!(
            manifest_url("bepolia"),
            "https://bera-snapshots.fsn1.your-objectstorage.com/v2/bepolia/reth/manifest.json"
        );
        assert_eq!(
            manifest_url("mainnet"),
            "https://bera-snapshots.fsn1.your-objectstorage.com/v2/mainnet/reth/manifest.json"
        );
    }

    #[test]
    fn injects_manifest_url_for_bare_download_with_known_chain() {
        let argv = vec!["bera-reth".into(), "download".into(), "--chain".into(), "bepolia".into()];
        let resolved = with_resolved_manifest_url(argv);
        assert_eq!(
            resolved.last().unwrap(),
            "https://bera-snapshots.fsn1.your-objectstorage.com/v2/bepolia/reth/manifest.json"
        );
        assert_eq!(resolved[resolved.len() - 2], "--manifest-url");
    }

    #[test]
    fn chain_arg_supports_equals_form() {
        let argv = vec!["bera-reth".into(), "download".into(), "--chain=mainnet".into()];
        assert_eq!(chain_arg(&argv), Some("mainnet".to_string()));
    }

    #[test]
    fn leaves_argv_alone_when_manifest_url_already_given() {
        let argv = vec![
            "bera-reth".into(),
            "download".into(),
            "--chain".into(),
            "bepolia".into(),
            "--manifest-url".into(),
            "https://example.com/manifest.json".into(),
        ];
        assert_eq!(with_resolved_manifest_url(argv.clone()), argv);
    }

    #[test]
    fn leaves_argv_alone_for_list_and_help() {
        let list = vec!["bera-reth".into(), "download".into(), "--list".into()];
        let help = vec!["bera-reth".into(), "download".into(), "--help".into()];
        assert_eq!(with_resolved_manifest_url(list.clone()), list);
        assert_eq!(with_resolved_manifest_url(help.clone()), help);
    }

    #[test]
    fn leaves_argv_alone_for_non_download_commands() {
        let argv = vec!["bera-reth".into(), "node".into(), "--chain".into(), "bepolia".into()];
        assert_eq!(with_resolved_manifest_url(argv.clone()), argv);
    }

    #[test]
    fn leaves_argv_alone_for_unknown_chain() {
        let argv = vec!["bera-reth".into(), "download".into(), "--chain".into(), "sepolia".into()];
        assert_eq!(with_resolved_manifest_url(argv.clone()), argv);
    }
}
