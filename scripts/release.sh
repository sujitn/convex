#!/bin/bash
set -e

VERSION=$1
if [ -z "$VERSION" ]; then
  echo "Usage: ./scripts/release.sh <version>"
  echo "Example: ./scripts/release.sh 0.2.0"
  exit 1
fi

echo "====================================="
echo "Convex Release Script v$VERSION"
echo "====================================="
echo ""

# Validate version format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: Version must be in format X.Y.Z (e.g., 0.2.0)"
  exit 1
fi

echo "Step 1: Building workspace..."
cargo build --all-features --workspace
echo "✓ Build successful"
echo ""

echo "Step 2: Running tests..."
cargo test --all-features --workspace
echo "✓ Tests passed"
echo ""

echo "Step 3: Checking formatting..."
cargo fmt --all -- --check
echo "✓ Formatting OK"
echo ""

echo "Step 4: Running Clippy..."
cargo clippy --all-features --workspace -- -D warnings
echo "✓ Clippy passed"
echo ""

echo "Step 5: Building documentation..."
cargo doc --no-deps --all-features --workspace
echo "✓ Documentation built"
echo ""

echo "Step 6: Dry run publish for all crates..."
CRATES="convex-core convex-math convex-curves convex-bonds convex-spreads convex-risk convex-yas"

for crate in $CRATES; do
  echo "  - Testing $crate..."
  cargo publish --dry-run -p $crate
done
echo "✓ All crates ready for publishing"
echo ""

echo "====================================="
echo "Validation Complete!"
echo "====================================="
echo ""
echo "All checks passed. Ready to release v$VERSION"
echo ""
echo "To complete the release, run:"
echo "  git tag v$VERSION"
echo "  git push origin v$VERSION"
echo ""
echo "This will trigger the release workflow which will:"
echo "  1. Validate the release"
echo "  2. Publish all crates to crates.io"
echo "  3. Create a GitHub release"
