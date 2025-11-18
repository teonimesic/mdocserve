# Release Process

This document describes the automated and manual release processes for mdocserve.

## Overview

The release process is fully automated with GitHub Actions:
1. **Version Bump** → Updates version in `package.json` and `Cargo.toml`
2. **Git Tag** → Creates and pushes a version tag
3. **Build** → Compiles binaries for all platforms (Linux, macOS, Windows)
4. **GitHub Release** → Creates release with all platform binaries
5. **npm Publish** → Automatically publishes to npm registry

## Automatic Releases (Patch Version)

Every push to `main` automatically triggers a **patch version bump**:
- `0.1.1` → `0.1.2` → `0.1.3` ...

This is ideal for:
- Bug fixes
- Small improvements
- Documentation updates
- Dependency updates

### Skipping Auto-Release

To push to `main` without triggering a release, add `[skip ci]` to your commit message:

```bash
git commit -m "Update documentation [skip ci]"
git push
```

## Manual Releases (Major/Minor/Patch)

Use manual releases when you need to bump **major** or **minor** versions.

### Version Bump Types

- **patch**: `0.1.1` → `0.1.2` (bug fixes, small changes)
- **minor**: `0.1.1` → `0.2.0` (new features, backwards compatible)
- **major**: `0.1.1` → `1.0.0` (breaking changes)

### Via GitHub UI

1. Go to [Actions → Version Bump & Release](https://github.com/teonimesic/mdocserve/actions/workflows/version-bump.yml)
2. Click **"Run workflow"** button (top right)
3. Select the bump type from dropdown:
   - `patch` - Bug fixes
   - `minor` - New features
   - `major` - Breaking changes
4. Click **"Run workflow"**

### Via GitHub CLI

```bash
# Bump minor version (new feature)
gh workflow run version-bump.yml -f bump_type=minor

# Bump major version (breaking change)
gh workflow run version-bump.yml -f bump_type=major

# Bump patch version manually
gh workflow run version-bump.yml -f bump_type=patch
```

## First-Time Setup

### NPM Token Configuration

The workflow requires an `NPM_TOKEN` secret to publish to npm automatically.

#### 1. Generate npm token

```bash
npm login  # if not already logged in
npm token create
```

Copy the token that's generated.

#### 2. Add to GitHub Secrets

1. Go to [Repository Settings → Secrets](https://github.com/teonimesic/mdocserve/settings/secrets/actions)
2. Click **"New repository secret"**
3. Name: `NPM_TOKEN`
4. Value: paste the token from step 1
5. Click **"Add secret"**

## Release Workflow Details

### Automated Steps

1. **version-bump.yml** (triggered on push to main or manual trigger)
   - Calculates new version
   - Updates `package.json` and `Cargo.toml`
   - Commits changes with `[skip ci]`
   - Creates and pushes git tag (e.g., `v0.1.2`)

2. **release.yml** (triggered by tag push)
   - Builds Rust binaries for all platforms:
     - Linux x64
     - Linux ARM64
     - macOS Intel (x64)
     - macOS Apple Silicon (ARM64)
     - Windows x64
   - Creates tar.gz/zip archives with platform-specific names
   - Creates GitHub release with all binaries
   - Publishes package to npm

### Build Matrix

The release workflow builds for these targets:

| Platform | Target Triple | Binary Name |
|----------|--------------|-------------|
| Linux x64 | `x86_64-unknown-linux-gnu` | `mdocserve-linux-x64` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `mdocserve-linux-arm64` |
| macOS Intel | `x86_64-apple-darwin` | `mdocserve-darwin-x64` |
| macOS ARM64 | `aarch64-apple-darwin` | `mdocserve-darwin-arm64` |
| Windows x64 | `x86_64-pc-windows-msvc` | `mdocserve-windows-x64.exe` |

## Monitoring Releases

### Check Workflow Status

View ongoing/recent releases:

```bash
# List recent workflow runs
gh run list --workflow=version-bump.yml --limit 5
gh run list --workflow=release.yml --limit 5

# Watch a specific run
gh run watch <run-id>
```

### Verify Release

After a release completes:

1. **GitHub Release**: Check https://github.com/teonimesic/mdocserve/releases
2. **npm**: Check https://www.npmjs.com/package/mdocserve
3. **Install test**:
   ```bash
   npm install -g mdocserve@latest
   mdocserve --version
   ```

## Troubleshooting

### Release Failed to Publish to npm

1. Check the [Actions logs](https://github.com/teonimesic/mdocserve/actions)
2. Verify `NPM_TOKEN` secret is set correctly
3. Ensure npm token hasn't expired
4. Re-run the failed workflow from GitHub UI

### Binary Build Failed

1. Check which platform failed in [Actions logs](https://github.com/teonimesic/mdocserve/actions)
2. Common issues:
   - Frontend build failure (check Node.js/npm versions)
   - Cross-compilation issues (ARM64 builds)
   - Rust compilation errors
3. Fix the issue and create a new release

### Version Conflict

If the version already exists on npm:

1. The publish step will fail
2. Manually bump version higher:
   ```bash
   # Trigger manual release with patch/minor/major
   gh workflow run version-bump.yml -f bump_type=patch
   ```

## Release Checklist

Before making a major/minor release:

- [ ] All tests passing locally (`cargo test`, `npm run test`)
- [ ] Documentation updated (README.md, docs/)
- [ ] CHANGELOG updated (if applicable)
- [ ] Breaking changes documented (for major releases)
- [ ] Choose appropriate version bump type (major/minor/patch)
- [ ] Trigger release manually or push to main

## Rolling Back a Release

If a release needs to be rolled back:

1. **npm**: Deprecate the version
   ```bash
   npm deprecate mdocserve@<version> "This version has issues, please upgrade"
   ```

2. **GitHub**: Mark release as pre-release or delete
   ```bash
   gh release delete v<version>
   ```

3. **Fix and re-release**: Create a new patch version with fixes
