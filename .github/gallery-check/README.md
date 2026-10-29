# The formula gallery's check

`.github/workflows/gallery-check.yml` runs on every pull request to this
repository and checks the formulas it adds to `gallery/`. This repository
publishes nothing: the site, gallery page included, is
[opendmd.dev](https://opendmd.dev/), built and deployed from OpenDMD's main
repository, which reads `gallery/` from here (anonymously) at each build.
Everything under `.github/` here (the workflow, `CODEOWNERS`, this file,
the checker's source in `opendmd-psi/` and `opendmd-gallery/`, and the
panel's fonts in `opendmd-core/src/fonts.rs`) is installed by OpenDMD's
`tools/gallery-repo-publish.sh` from the main repository
(`site/public-repo/`): change it there.

## What it checks

`opendmd-gallery pr` reads the pull request's merge commit from git's
object store (`git diff-tree`, `git cat-file`; nothing is checked out) and
fails unless every change:

- adds or changes one file `gallery/<slug>.psi`: no other path, no deletion,
  no subfolder, no symbolic link, submodule or executable file, at most 20
  files;
- has a slug of lower-case letters, digits and single hyphens (≤ 48);
- is at most 4 KB of UTF-8 text with LF line endings, no control characters
  (a tab only in the formula), no invisible, joining or direction-changing
  characters (the "Trojan Source" ones: U+202A–202E, U+2066–2069, …), no
  private-use ones, no more than two accents stacked on a letter;
- starts with the header: `-- author:` (≤ 40 characters, no `<` or `>`)
  and `-- licence:` (CC0-1.0 or CC-BY-4.0), then the formula, whose own
  `name "…"` (≤ 40) and `about "…"` (≤ 120) lines are required (read with
  the compiler, which refuses `" < > \` and hidden characters in quotes), a
  name and author that don't pass for OpenDMD or a maintainer (lookalike
  letters included); `-- bundle` (OpenDMD's examples only) is refused;
- compiles with OpenDMD's Psi compiler (the same crate the display runs) and
  costs at most 10 ms a frame on the display (`Program::esp32_us`).

It draws eight frames of each formula that passes into a PNG, kept as the
run's `formula-previews` artifact (14 days) for the reviewer, and writes a
summary on the run's page.

## Threat model

**Who:** anyone with a GitHub account can open a pull request, from a fork.
**What they control:** the whole pull request — the gallery file, and also
anything else in its tree, including its own copies of this workflow and of
the checker's source.

**How the check is set up** (the invariant is in a comment block at the top
of the workflow: nothing from the pull request is ever checked out, built
or run):

- Trigger `pull_request_target`: GitHub runs **main's** copy of the
  workflow, so a pull request that edits it changes nothing, and the check's
  result can be trusted as a required status check. (With `pull_request` a
  pull request runs its own copy and can make its own check pass.)
- `permissions: {}`: the job's token can do nothing. The repo is public, so
  the pull request is fetched anonymously; the workflow uses no secrets
  (the repository should have none).
- No checkout action. `refs/pull/<n>/merge` is fetched as git objects only,
  and its second parent must be the event's head SHA (else GitHub's merge
  is stale: the check asks the API for `mergeable`, fetches again, and
  otherwise fails asking for a re-run).
- The checker is built only from the merge's **first parent**, verified to
  be on main (`git archive refs/pr^1 .github/gallery-check`), never from the
  pull request's copy, with `cargo build --locked --offline`: it depends on
  nothing but its own source and OpenDMD's Psi crate, so nothing is
  downloaded and no code from the contribution is compiled.
- The contribution is only ever data: blobs read from git objects and
  parsed; a formula is compiled and evaluated by Psi's VM, which has no I/O
  (arithmetic over the pixel, the time and three sliders, a fixed-size
  program of at most 256 steps).
- Only the pull request's number and head SHA reach the scripts (through
  env, checked as digits and 40 hex characters) — never a title, branch name
  or anything else a contributor typed. Untrusted text is printed escaped
  (`{:?}`) in the log, so it cannot start a workflow command, and as HTML
  entities in the job summary.
- One action, `actions/upload-artifact`, pinned by full commit SHA. A
  10-minute timeout; one run per pull request at a time.

**What a malicious pull request can do:** spend a few minutes of CI; show
the reviewer a picture (drawn by the checker, not supplied) and escaped
text; propose a formula that is offensive, misleading or copied — only a
person can judge that, which is why every merge needs a maintainer's
review (CODEOWNERS).

**What it cannot do:** change what the check runs; get a token that can
write, or any secret; run its own code; reach the site without a
maintainer merging it. And after a merge, the site build (`tools/site.py`
in the main repository) checks every gallery file again with its own copy
of the checker, leaves out any that fail, never follows a link, and renders
every field as escaped text.

**The publish is the other way in.** `.github/`, `README.md`,
`gallery/README.md` and `dev/api/` are pushed to `main` by the main
repository's release workflow with a deploy key, which bypasses the
ruleset. Anyone who can run that workflow (or read its secrets) can rewrite
this repository, `gallery/` included, and so what the next site build
reads. So: keep the deploy key only in the main repository's secrets, put
the publish step behind a GitHub Environment with a required reviewer,
restrict who can push `v*` tags there, and if the main repository's secrets
ever leak, remove and replace the deploy key here (Settings › Deploy keys)
at once.

## Switching it on (the maintainers)

**This repository (opendmd/opendmd.github.io):**

1. Check `.github/CODEOWNERS` names the maintainers' real GitHub handles.
2. Settings › Rules › Rulesets, a new branch ruleset for `main`, active:
   - Require a pull request before merging: 1 approval; dismiss stale
     approvals when new commits are pushed; require approval of the most
     recent reviewable push; require review from Code Owners.
   - Require status checks to pass: "Gallery check (trusted)", from the
     source GitHub Actions; do **not** require branches to be up to date
     (the check already runs on the merge with main).
   - Require linear history (merge by squash).
   - Block force pushes; restrict deletions.
   - Bypass list: Deploy keys (the release's publish) and Repository admin only.
3. Settings › General: allow squash merging only; disable auto-merge.
4. Settings › Actions › General:
   - Allow only actions from this repository and `actions/upload-artifact@*`
     (the workflow pins it by commit SHA).
   - Fork pull request workflows: require approval for all external
     contributors.
   - Workflow permissions: read repository contents only; do not allow
     GitHub Actions to create or approve pull requests.
5. Settings › Secrets and variables › Actions: confirm there are none.
6. Settings › Pages: off (the site is opendmd.dev; this repository serves
   nothing).

**The main repository (opendmd/opendmd):** restrict who can push `v*` tags
(a tag ruleset), and consider an Environment with a required reviewer for
the release workflow's "Update the public gallery repo" step.

**Reviewing a contribution:** the check passed; only `gallery/*.psi`
changed; the preview looks right on a 128 × 32 panel; the name and
description are fine for everyone; the author owns it (or it is credited
properly); not a duplicate. Squash and merge; it is on the site from the
next site build (a release, or the main repository's Site workflow; the
build's log names any file the re-check left out).
