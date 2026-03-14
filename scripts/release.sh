#!/bin/bash
# Release helper for ai-rs: bumps version in Cargo.toml, commits, tags, and pushes.
#
# Usage:
#   ./scripts/release.sh patch          # 0.1.0 → 0.1.1
#   ./scripts/release.sh minor          # 0.1.0 → 0.2.0
#   ./scripts/release.sh major          # 0.1.0 → 1.0.0
#   ./scripts/release.sh 0.3.0          # explicit version
#   ./scripts/release.sh patch --dry    # preview only, no git changes
#
# What happens:
#   1. Bumps version in Cargo.toml
#   2. Runs `cargo check` to verify
#   3. Commits: "chore: release vX.Y.Z"
#   4. Tags: vX.Y.Z
#   5. Pushes commit + tag to origin
#   6. GitHub Actions (if configured) creates a release

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"

# --- Parse current version from Cargo.toml ---
current_version() {
  grep -m1 '^version = ' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/'
}

# --- Bump semver ---
bump_version() {
  local ver="$1" part="$2"
  local major minor patch
  IFS='.' read -r major minor patch <<< "$ver"
  case "$part" in
    major) echo "$((major + 1)).0.0" ;;
    minor) echo "$major.$((minor + 1)).0" ;;
    patch) echo "$major.$minor.$((patch + 1))" ;;
    *)     echo "$part" ;;  # treat as explicit version
  esac
}

# --- Usage ---
usage() {
  echo "Usage: $0 <patch|minor|major|VERSION> [--dry]"
  echo ""
  echo "  patch    Bump patch version (0.1.0 → 0.1.1)"
  echo "  minor    Bump minor version (0.1.0 → 0.2.0)"
  echo "  major    Bump major version (0.1.0 → 1.0.0)"
  echo "  VERSION  Set explicit version (e.g. 0.3.0)"
  echo "  --dry    Preview changes without committing or pushing"
  exit 1
}

# --- Args ---
BUMP=""
DRY_RUN=false

for arg in "$@"; do
  case "$arg" in
    --dry|--dry-run) DRY_RUN=true ;;
    -h|--help) usage ;;
    *)
      if [ -z "$BUMP" ]; then
        BUMP="$arg"
      else
        echo -e "${RED}Error: unexpected argument '$arg'${NC}"
        usage
      fi
      ;;
  esac
done

if [ -z "$BUMP" ]; then
  echo -e "${RED}Error: version or bump type required${NC}"
  usage
fi

# --- Calculate new version ---
OLD_VERSION=$(current_version)
NEW_VERSION=$(bump_version "$OLD_VERSION" "$BUMP")

# Validate semver
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$'; then
  echo -e "${RED}Error: '$NEW_VERSION' is not a valid semver version${NC}"
  exit 1
fi

echo -e "${CYAN}ai-rs release${NC}"
echo -e "  Current: ${YELLOW}$OLD_VERSION${NC}"
echo -e "  New:     ${GREEN}$NEW_VERSION${NC}"
echo ""

if [ "$DRY_RUN" = true ]; then
  echo -e "${YELLOW}[DRY RUN] No changes will be made${NC}"
  echo ""
fi

# --- 1. Update Cargo.toml ---
if [ "$DRY_RUN" = false ]; then
  # macOS sed doesn't support 0,/pat/ — use awk to replace only first match
  awk -v new="$NEW_VERSION" '!done && /^version = "/ { sub(/"[^"]*"/, "\"" new "\""); done=1 } 1' "$CARGO_TOML" > "$CARGO_TOML.tmp"
  mv "$CARGO_TOML.tmp" "$CARGO_TOML"
fi
echo -e "  ${GREEN}✓${NC} Cargo.toml: $OLD_VERSION → $NEW_VERSION"

# --- 2. Cargo check ---
if [ "$DRY_RUN" = false ]; then
  echo -e "  ${YELLOW}…${NC} Running cargo check..."
  cd "$PROJECT_ROOT"
  if ! cargo check --quiet 2>&1; then
    echo -e "  ${RED}✗${NC} cargo check failed — reverting Cargo.toml"
    git checkout "$CARGO_TOML"
    exit 1
  fi
  echo -e "  ${GREEN}✓${NC} cargo check passed"
fi

# --- 3. Commit + tag + push ---
if [ "$DRY_RUN" = false ]; then
  cd "$PROJECT_ROOT"
  git add Cargo.toml Cargo.lock
  git commit -m "chore: release v$NEW_VERSION"
  git tag "v$NEW_VERSION"
  echo -e "  ${GREEN}✓${NC} Committed and tagged v$NEW_VERSION"

  echo -e "  ${YELLOW}…${NC} Pushing to origin..."
  git push origin HEAD
  git push origin "v$NEW_VERSION"
  echo -e "  ${GREEN}✓${NC} Pushed to origin"
else
  echo ""
  echo -e "  Would commit: ${CYAN}chore: release v$NEW_VERSION${NC}"
  echo -e "  Would tag:    ${CYAN}v$NEW_VERSION${NC}"
  echo -e "  Would push:   ${CYAN}origin HEAD + v$NEW_VERSION${NC}"
fi

echo ""
echo -e "${GREEN}✓ Released ai-rs v$NEW_VERSION${NC}"
