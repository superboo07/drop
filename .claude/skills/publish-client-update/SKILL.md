---
name: publish-client-update
description: Upload newly built desktop client AppImages to a Drop server's Updater (Admin > Settings > Updater) with their tag, branch and drafted release notes, so clients get offered them. Use when the user asks to publish, ship, upload or release a client build/AppImage/update.
argument-hint: "[AppImage path(s)] [--draft] [--required]"
disable-model-invocation: true
allowed-tools: Bash(node .claude/skills/publish-client-update/publish.mjs *), Bash(git log *), Bash(git show *), Bash(git diff *), Bash(git rev-parse *), Bash(git cat-file *)
---

# Publish client update

Sends new desktop client AppImages to the Drop server's Updater, each with its tag, branch and release notes. The server side lives in `server/server/internal/clientreleases/` and `server/server/api/v1/admin/updater/`; this skill only drives that API through `publish.mjs` (in this folder), so don't reimplement the HTTP calls.

Arguments: `$ARGUMENTS`: optional AppImage paths (default: every AppImage in the repo root the server doesn't have yet), `--draft` to upload without publishing, `--required` to mark the release as a required update.

## 1. Find what's new

```bash
node .claude/skills/publish-client-update/publish.mjs status
```

It prints JSON with:
- `local`: each AppImage in the repo root, newest first, with `version`, `commit`, `dirty`, `branch` (test if dirty, else release), `sha256`, and `alreadyUploaded` when the server already has a release with that exact file.
- `latestByBranch`: the newest published release on each branch, including the `commit` from its tag.

If it prints an `error`:
- Missing `DROP_UPDATER_URL`: ask the user for their server's URL (e.g. `https://drop.example.lan`) and write it to `.claude/updater.env` as `DROP_UPDATER_URL=...`. That file is gitignored by the repo's `*.env` rule.
- Missing token or HTTP 403: give the user the `tokenLink` from the output. It opens the admin Tokens page with the name and the `updater:read` + `updater:new` permissions pre-filled. Ask them to create the token and either paste it to you (you write `DROP_UPDATER_TOKEN=...` into `.claude/updater.env`) or add it to the file themselves. Never echo the token back in chat or put it in a commit.

The builds to publish are the ones named in the arguments, or else every `local` entry with `alreadyUploaded: null`. If there are none, say so and stop. Builds are made by `bash desktop/build_appimage.sh`; don't run a build yourself unless the user asks.

## 2. Work out tag and branch

- **Tag**: the build's `version` exactly (e.g. `0.4.0-g1a2b3c4` or `0.4.0-g1a2b3c4.dirty`). It is the version the running client reports, and the AppImage's file name carries the same string. Don't shorten or prettify it.
- **Branch**: `test` for dirty builds, `release` otherwise (the `branch` field). Only override it if the user says so.

## 3. Draft release notes

Write the notes in Markdown from the commits since the base commit:

- **Base commit**: the commit of the last *release-branch* build, `latestByBranch.release.commit`. For a test build, use the newer of `latestByBranch.test.commit` and `latestByBranch.release.commit` instead (`git merge-base --is-ancestor` tells you which is newer), since test clients get both. If there's no base, use the previous `v*` tag (`git describe --tags --abbrev=0 <commit>`), or just summarise the build's own commit if that fails too.
- **Commits**: `git log --no-merges --format='%h %s' <base>..<commit> -- desktop/ libraries/base/ libraries/droplet_types/`. The client is built from these paths only; server-only commits don't belong in client notes. Read each commit's message, and `git show --stat` where the subject is unclear.

**Release builds: the notes must cover everything.** Every commit in that range must be accounted for, so someone going from the last release build to this one learns about every change. Don't drop a commit because it seems minor or internal. Group related commits into one bullet where they're really one change, and check afterwards that each commit in the list maps to a bullet. Use these sections, leaving out any that are empty:
- `## What's new`: features and behaviour changes
- `## Fixes`
- `## Under the hood`: refactors, build, packaging, dependency and performance work, in one plain line each

**Test builds** can be shorter: the player-visible changes are enough.

Either way, write plainly from the player's side ("Games with a Proton prefix launch faster", not "refactor umu launcher"), and don't include commit hashes.
- **Dirty builds** contain uncommitted changes that git history doesn't show. If `git rev-parse --short HEAD` matches the build's commit, look at `git diff --stat HEAD -- desktop/ libraries/base/` to describe what's in it. If it doesn't match, say in the notes that the build includes unreleased changes on top of `<commit>`. Either way, start the notes with a one-line "Test build" warning.

## 4. Confirm, then upload

Publishing puts the notes in front of every client on that branch, so show the user a summary first and wait for their go-ahead:

```
Drop Desktop Client_0.4.0-g1a2b3c4_amd64.AppImage
  tag: 0.4.0-g1a2b3c4 · branch: release · published (or: draft) · required: no
  covers: 14 commits, 9f02d1e..1a2b3c4 (since the last release build, 0.3.9-g9f02d1e)
  notes:
  <the notes>
```

Apply any edits they ask for. Then, for each build, write its notes to a temp file in your scratchpad and run:

```bash
node .claude/skills/publish-client-update/publish.mjs upload "<file>" \
  --tag "<tag>" --branch <release|test> --notes-file "<notes.md>" [--draft] [--required]
```

The upload streams the whole file (~150 MB), so give the command a timeout of several minutes. The server reads the architecture from the AppImage itself and rejects files that aren't type-2 AppImages. Report each result's `tag`, `branch`, and `page` (the release's admin page link). If an upload fails, show the error. Don't retry more than once.
