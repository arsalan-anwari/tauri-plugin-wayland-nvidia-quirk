#!/usr/bin/env bash
# Publishes the crate to crates.io.
#
#   ./publish.sh            # check everything, then ask before uploading
#   ./publish.sh --dry-run  # check everything, upload nothing
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

bold=$'\e[1m'; green=$'\e[32m'; red=$'\e[31m'; yellow=$'\e[33m'; dim=$'\e[2m'; reset=$'\e[0m'

dry_run=0
[ "${1:-}" = "--dry-run" ] && dry_run=1

step() { echo; echo "${bold}==>${reset} ${bold}$*${reset}"; }
die()  { echo "${red}✗${reset} $*" >&2; exit 1; }

field() { sed -n "/^\\[package\\]/,/^\\[/{s/^$1 *= *\"\\(.*\\)\"/\\1/p}" Cargo.toml | head -1; }
name="$(field name)"
version="$(field version)"
[ -n "$name" ] && [ -n "$version" ] || die "could not read name/version from Cargo.toml"

step "publishing ${name} ${version}"

step "working tree is clean"
[ -z "$(git status --porcelain)" ] || die "uncommitted changes; commit or stash them first"

step "this version is not already on crates.io"
published="$(curl -sS -H "User-Agent: ${name} publish.sh" \
  "https://crates.io/api/v1/crates/${name}/${version}" || true)"
case "$published" in
  *'"num":"'"${version}"'"'*) die "${name} ${version} is already published; bump the version in Cargo.toml" ;;
esac

step "fmt"
cargo fmt --all --check

step "clippy"
cargo clippy --all-targets -- -D warnings

step "tests"
cargo test --all-features

step "docs build the way docs.rs will"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features >/dev/null

step "packaging"
cargo package >/dev/null
files="$(cargo package --list)"

step "the demos stayed out of the crate"
if echo "$files" | grep -q '^demos/'; then
  echo "$files" | grep '^demos/' | sed 's/^/  /'
  die "demo files are in the package; fix \`include\` in Cargo.toml"
fi
echo "$files" | sed 's/^/  /'
echo "${green}✓${reset} $(echo "$files" | wc -l) files, none from demos/"

step "CHANGELOG mentions ${version}"
grep -q "\[${version}\]" CHANGELOG.md || die "no [${version}] section in CHANGELOG.md"

if [ "$dry_run" = 1 ]; then
  step "dry run"
  cargo publish --dry-run
  echo; echo "${green}${bold}✓ ready${reset}, rerun without --dry-run to upload."
  exit 0
fi

echo
echo "${yellow}${bold}About to publish ${name} ${version} to crates.io.${reset}"
echo "${dim}This is permanent, a published version can be yanked, never removed.${reset}"
read -r -p "Type the version to confirm: " confirm
[ "$confirm" = "$version" ] || die "aborted"

step "publishing"
cargo publish

step "tagging"
if git rev-parse "v${version}" >/dev/null 2>&1; then
  echo "${dim}tag v${version} already exists${reset}"
else
  git tag -a "v${version}" -m "${name} ${version}"
  echo "${green}✓${reset} tagged v${version} ${dim}(push it: git push origin v${version})${reset}"
fi

echo
echo "${green}${bold}✓ published${reset} https://crates.io/crates/${name}/${version}"
