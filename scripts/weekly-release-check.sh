#!/usr/bin/env bash
# Weekly release check: compare origin/main with the latest v* tag and report
# whether a release is needed. Read-only: no tag, commit, or release is created.
set -euo pipefail

REMOTE="${REMOTE:-origin}"
BRANCH="${BRANCH:-main}"

git fetch --tags --prune "$REMOTE" >/dev/null

latest="$(git tag --list 'v*' --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -1)"
if [ -z "$latest" ]; then
  echo "no stable vX.Y.Z tag found; create the first release."
  exit 0
fi

tag_epoch="$(git log -1 --format=%ct "$latest")"
now_epoch="$(date +%s)"
age_days="$(( (now_epoch - tag_epoch) / 86400 ))"

echo "latest tag : ${latest} ($(git log -1 --format=%cI "$latest"))"
echo "tag age    : ${age_days} day(s)"
echo "compare    : ${REMOTE}/${BRANCH}"
echo

commits="$(git log --format='%h %cI %s' "${latest}..${REMOTE}/${BRANCH}")"
if [ -z "$commits" ]; then
  echo "result: no commits since ${latest}; no release needed."
  exit 0
fi

echo "commits since ${latest}:"
echo "$commits" | sed 's/^/  /'
echo

files="$(git diff --name-only "${latest}..${REMOTE}/${BRANCH}")"
echo "changed paths:"
echo "$files" | sed 's/^/  /'
echo

release_relevant=0
while IFS= read -r f; do
  case "$f" in
    src/*|tests/*|Cargo.toml|Cargo.lock|build.rs|skills/*|install.sh)
      release_relevant=1
      ;;
  esac
done <<< "$files"

if [ "$release_relevant" -eq 1 ]; then
  echo "result: release-relevant changes present (code/tests/installer/embedded skill)."
  echo "action: classify as patch/minor/major per docs/RELEASING.md and cut a release."
else
  echo "result: only docs/CI/metadata changed; no binary or installer change detected."
  echo "action: batch with the next code release, or cut a patch release if distribution changed."
fi
