# The formula gallery

Formulas for the [OpenDMD](https://opendmd.github.io/) LED display, shown at
**[opendmd.github.io/formulas](https://opendmd.github.io/formulas/)**. A
formula is a few lines of maths that draws on the 128 × 32 display, every
pixel at once ([how formulas work](https://opendmd.github.io/guide/formulas.html)).
Anyone can add one with a pull request.

## The easy way

1. Make a formula in the web app's **Formulas** (on your display, or in the
   [demo](https://opendmd.github.io/demo/#formulas)).
2. **Copy share link**, open the link, and press **Submit to the gallery**.
3. Fill in a description and your name, pick a licence, and press **Open on
   GitHub**: GitHub opens with the file ready. Press **Propose changes**,
   then **Create pull request**. (GitHub makes a copy of this repository in
   your account for it; you need a free GitHub account.)

## By hand

Add one file, `gallery/<name>.psi`, with this header and then your formula:

```text
-- name: Rainbow rings
-- description: Rings rippling out from the middle
-- author: @your-github-name
-- licence: CC0-1.0
loop 4
d = len((x - 0.5)*4, y - 0.5)
pal(rainbow, frac(d*1.5 - t/2))
```

- **The file name**: lower-case letters, digits and single hyphens, then
  `.psi` (`rainbow-rings.psi`), at most 48 characters before `.psi`.
- **name**: at most 40 characters. **description**: one line, at most 120.
  **author**: your GitHub handle or your name, at most 40. Plain text: no
  `<` or `>`.
- **licence**: `CC0-1.0` (anyone may use it for anything, no credit
  needed: the default) or `CC-BY-4.0` (anyone may use it, crediting you).
  By submitting you confirm you made the formula, or may share it under
  that licence.
- An optional fifth line sets where the sliders start:
  `-- values: a=0.3 b=1` (each 0 to 1).
- The formula must work, and be light enough for the display: at most 8 ms
  a frame (the editor says "light" or "medium", not "heavy").
- The whole file at most 4 KB.

One formula per file; one or a few files per pull request; only files in
`gallery/`.

## What happens next

A check runs on your pull request and says on its page what, if anything,
to fix (push a fix to the same branch and it runs again). Then a
maintainer looks at it: that it plays well on the display, that it is
suitable for everyone, and that it isn't a copy of one already here. Once
merged, it appears on the gallery with the next update of the site.

Maintainers may decline or remove a formula without giving a reason. The
rest of this repository is the published site, built from OpenDMD's main
repository: pull requests that change anything outside `gallery/` are not
accepted.
