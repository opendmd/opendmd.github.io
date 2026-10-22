# The formula gallery's check and the site's deploy

`.github/workflows/gallery-check.yml` runs on every pull request to this
repository and checks the formulas it adds to `gallery/`.
`.github/workflows/pages.yml` deploys the site to GitHub Pages on every push
to `main`, with the gallery's page regenerated from `gallery/`, so a merged
formula is live within minutes. Everything under `.github/` here (the
workflows, `CODEOWNERS`, this file, the checker's source in `opendmd-psi/`
and `opendmd-gallery/`, and the gallery page's generator in
`tools/gallery_page.py`) is installed by OpenDMD's `tools/site-publish.sh`
from the main repository (`site/public-repo/`, `tools/`): change it there.

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
- starts with the header: `-- name:` (≤ 40 characters), `-- description:`
  (≤ 120), `-- author:` (≤ 40), `-- licence:` (CC0-1.0 or CC-BY-4.0), and
  optionally `-- values:` (`a=0.3 b=1`), with no `<` or `>` in them, and a
  name and author that don't pass for OpenDMD or a maintainer (lookalike
  letters included);
- compiles with OpenDMD's Psi compiler (the same crate the display runs) and
  costs at most 8 ms a frame on the display (`Program::esp32_us`).

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
maintainer merging it. And after a merge, the deploy (below) and the site
build (`tools/site.py` in the main repository) check every gallery file
again with main's copy of the checker, leave out any that fail, never
follow a link, and render every field as escaped text.

## The deploy (pages.yml)

The site is served by GitHub Pages from **GitHub Actions** (not from the
branch): `pages.yml` runs on every push to `main` (a merged pull request,
or the release's publish) and when a maintainer runs it by hand
(`workflow_dispatch`). Its build job fetches `main` anonymously (git
objects, `git archive`; no checkout action), builds main's vendored checker
(`cargo build --locked --offline`, as the check does), and runs main's
vendored `tools/gallery_page.py` (Python's standard library only, `python3
-I`) over the site: it checks every `gallery/*.psi` again, draws the
thumbnails and rewrites the part of `formulas/index.html` between its
`<!-- gallery:begin -->` and `<!-- gallery:end -->` markers. That is the
same code the release's site build runs, so given the same gallery the
deployed page is byte for byte the one the release built. The artifact is
the repository's files without `.github/` and `gallery/` (the formulas'
sources are not served; their page is), and without anything that is not
a plain file or folder (a symbolic link is deleted, never followed). The
deploy job runs only `actions/deploy-pages`.

**What it can do:** publish what is on `main`. A formula is live within
minutes of its merge, so the merge (the maintainer's review) is the gate,
as before; the check and the re-check still keep out a file that fails.

**What it cannot do:** run on a pull request (it has no `pull_request*`
trigger, and a fork's pull request could only ever get a read-only token
and no `id-token`); deploy from a branch other than `main` (the
`github-pages` environment only accepts `main`, and both jobs also check
`github.ref`); write to the repository (the build's token is read-only and
unused, the deploy's is `pages: write` + `id-token: write` only); read a
secret (there are none). Untrusted text never reaches a script through an
expression, and the step that reads gallery files first stops workflow
commands (`::stop-commands::`), so nothing in a file can be read as one.

**Who can deploy:** anyone who can get content onto `main` — a maintainer
merging a reviewed pull request, a repository admin (bypass), the release's
deploy key — or run the workflow on `main` by hand. Someone with write
access can also push a branch, and a branch can carry its own workflows
(run on push or by hand): the `github-pages` environment refuses a
deployment from it, but keep write access to the maintainers all the same.

**If Actions is unavailable** (an outage, or the deploy breaks): switch
Pages back to the branch (Settings › Pages › Build and deployment › Source:
Deploy from a branch, `main`, `/`) and disable `pages.yml`. The branch holds
the whole site as the last release built it, gallery page included (as of
that release), so the site keeps working; formulas merged since then appear
at the next release, or when the deploy is switched back on. The deployed
site stays up while the source is switched either way.

**The publish is the other way in.** The site is pushed to `main` by the
main repository's release workflow with a deploy key, which bypasses the
ruleset. Anyone who can run that workflow (or read its secrets) can rewrite
the site and `gallery/`. So: keep the deploy key only in the main
repository's secrets, put the publish step behind a GitHub Environment with
a required reviewer, restrict who can push `v*` tags there, and if the main
repository's secrets ever leak, remove and replace the deploy key here
(Settings › Deploy keys) at once.

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
   - Bypass list: Deploy keys (the site publish) and Repository admin only.
3. Settings › General: allow squash merging only; disable auto-merge.
4. Settings › Actions › General:
   - Allow only actions from this repository and `actions/upload-artifact@*`,
     `actions/upload-pages-artifact@*` and `actions/deploy-pages@*` (the
     workflows pin each by commit SHA; `upload-pages-artifact` runs
     `upload-artifact` itself).
   - Fork pull request workflows: require approval for all external
     contributors.
   - Workflow permissions: read repository contents only; do not allow
     GitHub Actions to create or approve pull requests.
5. Settings › Secrets and variables › Actions: confirm there are none.
6. Settings › Environments › `github-pages`: Deployment branches and tags:
   selected branches, `main` only; no other rule is needed (no secrets).
7. Settings › Pages: Build and deployment, Source: **GitHub Actions**;
   Enforce HTTPS. (Only after a site publish has installed `pages.yml` and
   given `formulas/index.html` its gallery markers: then run the Pages
   workflow once by hand and check the site. Until then, deploy from branch
   `main`, folder `/`: the fallback above.)

**The main repository (opendmd/opendmd):** restrict who can push `v*` tags
(a tag ruleset), and consider an Environment with a required reviewer for
the release workflow's "Publish the site" step.

**Reviewing a contribution:** the check passed; only `gallery/*.psi`
changed; the preview looks right on a 128 × 32 panel; the name and
description are fine for everyone; the author owns it (or it is credited
properly); not a duplicate. Squash and merge; the Pages workflow deploys it
within minutes (Actions › Pages shows the run; its log names any file the
re-check left out).
