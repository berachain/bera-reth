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

use std::ffi::{OsStr, OsString};

const PUBLIC_BASE: &str = "https://bera-snapshots.fsn1.your-objectstorage.com";

/// Chains this resolver knows how to look up. Mirrors `chainspec::mod.rs`'s
/// own `BERACHAIN_MAINNET`/`BERACHAIN_BEPOLIA` chain-name strings, but
/// duplicated rather than imported: those live in the library crate, this
/// module lives in the binary crate, and reusing them would mean exposing
/// them as public library API just to de-duplicate two literals in one
/// bin-only file. Any other `--chain` value (a custom genesis file path,
/// for example) is left untouched.
const KNOWN_CHAINS: &[&str] = &["mainnet", "bepolia"];

/// Boolean short flags on `download` that clap lets an operator cluster in
/// front of `-u` in a single token (`-yu <URL>`, `-vvyu<URL>`): `-y`
/// (`--non-interactive`) plus the global `-v`/`-q` display flags.
const CLUSTERABLE_BOOL_SHORTS: &[char] = &['y', 'v', 'q'];

/// Rewrites `argv` to inject `--manifest-url` when the caller ran
/// `download --chain <mainnet|bepolia>` without supplying a manifest source
/// of their own. Returns `argv` unchanged for every other invocation:
/// commands whose subcommand is not `download`, a download that already
/// names a manifest source, an unrecognized `--chain` value, or
/// `--list`/`--help`.
///
/// Operates on `OsString` so that arguments which are not valid UTF-8 (a
/// non-UTF-8 datadir or custom genesis path, which clap accepts via
/// `args_os`) pass through untouched instead of aborting the process. Such
/// tokens can never spell `download`, `--chain`, or a manifest-source flag,
/// so they are simply never matched.
pub(crate) fn with_resolved_manifest_url(argv: Vec<OsString>) -> Vec<OsString> {
    let Some(download_args) = download_subcommand_args(&argv) else {
        return argv;
    };

    if has_explicit_manifest_source(download_args) || is_list_or_help(download_args) {
        return argv;
    }

    let Some(chain) = chain_arg(download_args) else {
        return argv;
    };

    if !KNOWN_CHAINS.contains(&chain) {
        return argv;
    }

    let url = manifest_url(chain);
    let mut argv = argv;
    argv.push("--manifest-url".into());
    argv.push(url.into());
    argv
}

/// The fixed, in-place-overwritten manifest URL for a known chain.
fn manifest_url(chain: &str) -> String {
    format!("{PUBLIC_BASE}/v2/{chain}/reth/manifest.json")
}

/// Whether an argument token begins with `-`, i.e. is an option rather than
/// a subcommand or value. Checked at the byte level so that non-UTF-8 tokens
/// are classified too: `OsStr::as_encoded_bytes` guarantees ASCII is
/// preserved on every platform.
fn is_option_like(arg: &OsStr) -> bool {
    arg.as_encoded_bytes().starts_with(b"-")
}

/// Returns the arguments following the `download` subcommand token, or
/// `None` if `download` is not the subcommand being invoked.
///
/// The subcommand is the first non-option token after the binary name; reth's
/// global options (`-vvv`, `--quiet`, `--log.*=...`, `--color=...`) may
/// legitimately precede it. A `download` token appearing anywhere else, e.g.
/// as an option value in `node --datadir download`, is not a subcommand and
/// must not trigger injection, since clap would then reject `--manifest-url`
/// on a command that does not define it.
///
/// A value-taking global option written in separated form before the
/// subcommand (`--color never download ...`) makes the value look like the
/// subcommand, so no injection happens; the operator sees the normal
/// upstream error and can either pass `--manifest-url` or write the option
/// as `--color=never`. Failing closed here is deliberate.
fn download_subcommand_args(argv: &[OsString]) -> Option<&[OsString]> {
    let subcommand_index = argv.iter().skip(1).position(|a| !is_option_like(a))? + 1;
    (argv[subcommand_index] == "download").then(|| &argv[subcommand_index + 1..])
}

/// Whether the operator already named a manifest source, in any of the
/// spellings clap accepts for `DownloadCommand`'s `--manifest-url`,
/// `--manifest-path`, and `-u`/`--url` options.
fn has_explicit_manifest_source(download_args: &[OsString]) -> bool {
    download_args.iter().filter_map(|a| a.to_str()).any(|a| {
        a == "--manifest-url" ||
            a.starts_with("--manifest-url=") ||
            a == "--manifest-path" ||
            a.starts_with("--manifest-path=") ||
            a == "--url" ||
            a.starts_with("--url=") ||
            is_short_url_option(a)
    })
}

/// Matches every form clap accepts for the short `-u` option: `-u <URL>`,
/// `-u=<URL>`, `-u<URL>`, and `-u` clustered behind boolean shorts such as
/// `-yu <URL>` or `-yu<URL>`.
fn is_short_url_option(arg: &str) -> bool {
    let Some(shorts) = arg.strip_prefix('-') else {
        return false;
    };
    if shorts.starts_with('-') {
        return false;
    }
    shorts.trim_start_matches(CLUSTERABLE_BOOL_SHORTS).starts_with('u')
}

/// `--list` and `--help` never need a manifest source: `--list` conflicts
/// with one in clap, and `--help` exits before any download runs.
fn is_list_or_help(download_args: &[OsString]) -> bool {
    download_args
        .iter()
        .any(|a| a == "--list" || a == "--list-snapshots" || a == "-h" || a == "--help")
}

/// Extracts the value of `--chain <value>` or `--chain=<value>`.
///
/// Returns `None` for a non-UTF-8 value: that can only be a custom genesis
/// path, which is never one of [`KNOWN_CHAINS`] and so needs no lookup.
fn chain_arg(download_args: &[OsString]) -> Option<&str> {
    for (i, arg) in download_args.iter().enumerate() {
        if let Some(value) = arg.to_str().and_then(|a| a.strip_prefix("--chain=")) {
            return Some(value);
        }
        if arg == "--chain" {
            return download_args.get(i + 1).filter(|v| !is_option_like(v))?.to_str();
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
    fn injects_when_global_options_precede_download_subcommand() {
        let argv: Vec<OsString> = vec![
            "bera-reth".into(),
            "-vvv".into(),
            "--color=never".into(),
            "--log.file.directory=/tmp/logs".into(),
            "download".into(),
            "--chain".into(),
            "mainnet".into(),
        ];
        let resolved = with_resolved_manifest_url(argv);
        assert_eq!(
            resolved.last().unwrap(),
            "https://bera-snapshots.fsn1.your-objectstorage.com/v2/mainnet/reth/manifest.json"
        );
    }

    #[test]
    fn chain_arg_supports_equals_form() {
        let argv: Vec<OsString> =
            vec!["bera-reth".into(), "download".into(), "--chain=mainnet".into()];
        assert_eq!(chain_arg(download_subcommand_args(&argv).unwrap()), Some("mainnet"));
    }

    #[test]
    fn chain_arg_ignores_missing_value() {
        let argv: Vec<OsString> = vec!["download".into(), "--chain".into(), "--minimal".into()];
        assert_eq!(chain_arg(&argv[1..]), None);
    }

    #[cfg(unix)]
    #[test]
    fn passes_non_utf8_arguments_through_untouched() {
        use std::os::unix::ffi::OsStrExt;

        let non_utf8 = OsStr::from_bytes(b"/data/gen\xff\xfesis.json").to_os_string();
        assert!(non_utf8.to_str().is_none(), "fixture must not be valid UTF-8");

        // A non-UTF-8 custom genesis path is never a known chain: no injection,
        // and crucially no panic anywhere in the scan.
        let genesis: Vec<OsString> =
            vec!["bera-reth".into(), "download".into(), "--chain".into(), non_utf8.clone()];
        assert_eq!(with_resolved_manifest_url(genesis.clone()), genesis);

        // A non-UTF-8 value elsewhere on a `download` must not block a
        // known-chain lookup.
        let datadir: Vec<OsString> = vec![
            "bera-reth".into(),
            "download".into(),
            "--datadir".into(),
            non_utf8.clone(),
            "--chain".into(),
            "bepolia".into(),
        ];
        let resolved = with_resolved_manifest_url(datadir);
        assert_eq!(
            resolved.last().unwrap(),
            "https://bera-snapshots.fsn1.your-objectstorage.com/v2/bepolia/reth/manifest.json"
        );

        // And a non-UTF-8 value on a non-`download` command is simply not ours.
        let node: Vec<OsString> =
            vec!["bera-reth".into(), "node".into(), "--datadir".into(), non_utf8];
        assert_eq!(with_resolved_manifest_url(node.clone()), node);
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
    fn leaves_argv_alone_for_every_short_url_spelling() {
        let base: Vec<OsString> =
            vec!["bera-reth".into(), "download".into(), "--chain".into(), "bepolia".into()];
        let spellings: [&[&str]; 6] = [
            &["-u", "https://example.com/snap.tar.lz4"],
            &["-u=https://example.com/snap.tar.lz4"],
            &["-uhttps://example.com/snap.tar.lz4"],
            &["-yu", "https://example.com/snap.tar.lz4"],
            &["-yu=https://example.com/snap.tar.lz4"],
            &["-vvyuhttps://example.com/snap.tar.lz4"],
        ];
        for spelling in spellings {
            let mut argv = base.clone();
            argv.extend(spelling.iter().map(OsString::from));
            assert_eq!(with_resolved_manifest_url(argv.clone()), argv, "spelling: {spelling:?}");
        }
    }

    #[test]
    fn short_url_detection_does_not_match_other_short_flags() {
        for flag in ["-y", "-vvv", "-q", "-h", "--url-ish"] {
            assert!(!is_short_url_option(flag), "flag: {flag}");
        }
    }

    #[test]
    fn leaves_argv_alone_when_download_is_an_option_value_not_the_subcommand() {
        let argv = vec![
            "bera-reth".into(),
            "node".into(),
            "--datadir".into(),
            "download".into(),
            "--chain".into(),
            "bepolia".into(),
        ];
        assert_eq!(with_resolved_manifest_url(argv.clone()), argv);
    }

    #[test]
    fn fails_closed_when_separated_global_value_precedes_download() {
        // `never` is what the scan sees as the subcommand, so no injection.
        let argv = vec![
            "bera-reth".into(),
            "--color".into(),
            "never".into(),
            "download".into(),
            "--chain".into(),
            "bepolia".into(),
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
