# The formula gallery

Formulas for the [OpenDMD](https://opendmd.dev/) LED display, shown at
**[opendmd.dev/formulas](https://opendmd.dev/formulas/)**. A
formula is a few lines of maths that draws on the 128 × 32 display, every
pixel at once ([how formulas work](https://opendmd.dev/guide/formulas)).
Anyone can add one with a pull request.

## The easy way

1. Make a formula in the web app's **Formulas** (on your display, or in the
   [demo](https://opendmd.dev/demo/#formulas)).
2. **Copy share link**, open the link, and press **Submit to the gallery**.
3. Fill in its name, a line about it and your name, pick a licence, and
   press **Open on GitHub**: GitHub opens with the file ready. Press **Propose changes**,
   then **Create pull request**. (GitHub makes a copy of this repository in
   your account for it; you need a free GitHub account.)

## By hand

Add one file, `gallery/<name>.psi`: two header lines, then your formula,
which starts by saying its name and what it is:

```text
-- author: @your-github-name
-- licence: CC0-1.0
name "Rainbow rings"
about "Rings rippling out from the middle"
control width 0..1 = 0.3 "Ring width"
loop 4
d = len((x - 0.5)*4, y - 0.5)
pal(rainbow, frac(d*(1 + width*2) - t/2))
```

- **The file name**: lower-case letters, digits and single hyphens, then
  `.psi` (`rainbow-rings.psi`), at most 48 characters before `.psi`.
- **author**: your GitHub handle or your name, at most 40. Plain text: no
  `<` or `>`.
- **`name "…"`**: at most 40 characters. **`about "…"`**: one line, at most
  120. They are the formula's own first lines (so the web app, the panel
  and the gallery all read the same thing); no `"`, `<`, `>` or `\` in
  them.
- **licence**: `CC0-1.0` (anyone may use it for anything, no credit
  needed: the default) or `CC-BY-4.0` (anyone may use it, crediting you).
  By submitting you confirm you made the formula, or may share it under
  that licence.
- Its sliders are its `control` lines (up to three), and they start at
  the defaults written there (`= 0.3`). A `look` line says it can be a
  panel's look, drawn over every GIF (see
  [how formulas work](https://opendmd.dev/guide/formulas.html)).
- The formula must work, and be light enough for the display: at most 10 ms
  a frame as the editor estimates it.
- The whole file at most 4 KB.

One formula per file; one or a few files per pull request; only files in
`gallery/`.

## What happens next

A check runs on your pull request and says on its page what, if anything,
to fix (push a fix to the same branch and it runs again). Then a
maintainer looks at it: that it plays well on the display, that it is
suitable for everyone, and that it isn't a copy of one already here. Once
merged, it appears on the gallery when the site is next updated.

Maintainers may decline or remove a formula without giving a reason. The
rest of this repository is written from OpenDMD's main repository: pull
requests that change anything outside `gallery/` are not accepted.
