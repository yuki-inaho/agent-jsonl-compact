#!/usr/bin/env bash
# Verify that living docs exist in Japanese/English pairs and link to each other.
set -euo pipefail

fail=0

check_pair() {
  ja="$1"
  en="$2"
  if [ ! -f "$ja" ]; then
    echo "missing: $ja"
    fail=1
  fi
  if [ ! -f "$en" ]; then
    echo "missing: $en"
    fail=1
  fi
  if [ -f "$ja" ] && ! grep -q "$(basename "$en")" "$ja"; then
    echo "$ja does not link to $(basename "$en")"
    fail=1
  fi
  if [ -f "$en" ] && ! grep -q "$(basename "$ja")" "$en"; then
    echo "$en does not link to $(basename "$ja")"
    fail=1
  fi
}

check_pair README.md README.en.md
check_pair SECURITY.md SECURITY.en.md
check_pair CHANGELOG.md CHANGELOG.ja.md
check_pair docs/README.md docs/README.en.md
check_pair docs/CLI.md docs/CLI.en.md
check_pair docs/INPUT_FORMATS.md docs/INPUT_FORMATS.en.md
check_pair docs/ONBOARDING.md docs/ONBOARDING.en.md
check_pair docs/RELEASING.md docs/RELEASING.en.md
check_pair docs/adr/README.md docs/adr/README.en.md
check_pair docs/adr/0004-versioning-and-release-policy.md docs/adr/0004-versioning-and-release-policy.en.md
check_pair docs/adr/0005-bilingual-documentation.md docs/adr/0005-bilingual-documentation.en.md
check_pair docs/adr/0006-safe-installation-and-storage.md docs/adr/0006-safe-installation-and-storage.en.md
check_pair docs/adr/0007-untrusted-log-evidence.md docs/adr/0007-untrusted-log-evidence.en.md

if [ "$fail" -ne 0 ]; then
  echo "docs language pairs: FAILED"
  exit 1
fi
echo "docs language pairs: OK"
