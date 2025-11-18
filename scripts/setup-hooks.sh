#!/bin/bash
# Setup git hooks for mdocserve development

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HOOKS_DIR="$PROJECT_ROOT/.githooks"
GIT_HOOKS_DIR="$PROJECT_ROOT/.git/hooks"

echo "🔧 Setting up git hooks for mdocserve..."

# Check if .git directory exists
if [ ! -d "$GIT_HOOKS_DIR" ]; then
    echo "❌ Error: .git/hooks directory not found. Are you in a git repository?"
    exit 1
fi

# Configure git to use .githooks directory
echo "📝 Configuring git to use .githooks directory..."
git config core.hooksPath .githooks

# Make hooks executable
echo "🔑 Making hooks executable..."
chmod +x "$HOOKS_DIR"/*

echo "✅ Git hooks installed successfully!"
echo ""
echo "Pre-commit hook will now run:"
echo "  - prettier --check (Frontend formatting)"
echo "  - eslint (Frontend linting)"
echo "  - tsc --noEmit (Frontend type checking)"
echo "  - npm test (Frontend tests, if frontend files changed)"
echo "  - cargo fmt --check (Rust formatting)"
echo "  - cargo clippy (Rust linting)"
echo ""
echo "Pre-push hook will now run:"
echo "  - All frontend tests (npm test)"
echo "  - All backend tests (cargo test)"
echo ""
echo "To skip hooks temporarily, use:"
echo "  git commit --no-verify  (skip pre-commit)"
echo "  git push --no-verify    (skip pre-push)"
