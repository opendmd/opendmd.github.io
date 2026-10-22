"""The formula gallery's page (/formulas/ on the site): its thumbnails and
the part of formulas/index.html between the gallery markers.

One implementation, run in two places:

- the site build: tools/site.py imports it (`build`, `marked`) and writes
  the page around it;
- the public site repo's Pages workflow (.github/workflows/pages.yml there),
  on every push to its main: tools/site-publish.sh vendors this file to
  `.github/gallery-check/tools/gallery_page.py`, and the workflow runs it
  over the published site with the gallery as merged, so a merged
  contribution is live without a release:

      python3 -I gallery_page.py SITE --tool opendmd-gallery [--gallery DIR]

  rewrites SITE/formulas/ (the thumbnails, and the page between its
  markers) from SITE/demo/formulas.json (the examples, as the site build
  copied them) and DIR/*.psi (the contributions).

Given the same examples, contributions and checker, both write the same
bytes (tools/gallery-pages-test.py checks it). The Python standard library
only, nothing else: the workflow runs it with the runner's python3, and it
must not import anything from the site it rewrites (`-I`).
"""

from __future__ import annotations

import argparse
import html
import json
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import unicodedata
import zlib
from pathlib import Path

PAGE = "formulas/index.html"
REPO = "https://github.com/opendmd/opendmd.github.io"
LICENCE_LINKS = {"CC0-1.0": ("CC0", "https://creativecommons.org/publicdomain/zero/1.0/"),
                 "CC-BY-4.0": ("CC BY 4.0", "https://creativecommons.org/licenses/by/4.0/")}
# More files than this in gallery/ and the rest are left out (a sanity limit).
MAX = 2000
# The generated part of formulas/index.html. Nothing inside can contain
# either marker: every field is escaped (no `<`), and the checker refuses
# `<` and `>` in a header anyway.
BEGIN, END = "<!-- gallery:begin -->", "<!-- gallery:end -->"


def urlencoded(pairs: list[tuple[str, str]]) -> str:
    """application/x-www-form-urlencoded exactly as the browser's
    URLSearchParams writes it (alphanumerics and *-._ kept, space as +), so
    the gallery's links are the web app's (js/psikit.js `link`)."""
    def enc(v: str) -> str:
        out = []
        for b in v.encode():
            c = chr(b)
            out.append(c if (c.isascii() and c.isalnum()) or c in "*-._" else "+" if c == " " else f"%{b:02X}")
        return "".join(out)
    return "&".join(f"{enc(k)}={enc(v)}" for k, v in pairs)


def share_hash(e: dict, name: str | None = None) -> str:
    """`#formula=…&name=…&a=…`: only the sliders the formula reads that are
    not 0, as `+v.toFixed(2)` writes them."""
    pairs = [("formula", e["src"]), ("name", name if name is not None else e["name"])]
    for j, k in enumerate("abc"):
        if e["inputs"] & (1 << j) and e.get(k):
            pairs.append((k, f"{e[k]:.2f}".rstrip("0").rstrip(".")))
    return "#" + urlencoded(pairs)


def slugify(name: str) -> str:
    s = unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode().lower()
    return re.sub(r"[^a-z0-9]+", "-", s).strip("-")[:48].strip("-")


def recompress_png(data: bytes) -> bytes:
    """opendmd-gallery writes stored (uncompressed) PNGs: the same image,
    deflated."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    chunks, idat, i = [], b"", 8
    while i < len(data):
        n, kind = struct.unpack(">I4s", data[i:i + 8])
        body = data[i + 8:i + 8 + n]
        i += 12 + n
        if kind == b"IDAT":
            idat += body
        else:
            chunks.append((kind, body))
    out = b"\x89PNG\r\n\x1a\n"
    for kind, body in chunks:
        if kind == b"IEND":
            z = zlib.compress(zlib.decompress(idat), 9)
            out += struct.pack(">I", len(z)) + b"IDAT" + z + struct.pack(">I", zlib.crc32(b"IDAT" + z))
        out += struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    return out


def say(msg: str) -> None:
    """A line on stderr, on one line whatever a file name holds."""
    print(msg.replace("\r", " ").replace("\n", " "), file=sys.stderr)


def check_gallery(tool: Path, files: list[Path], thumbs: Path, trusted: bool = False) -> tuple[list[dict], list[dict]]:
    """opendmd-gallery check over `files`: (entries, failures). If the
    checker itself fails on a batch (not a verdict: a crash), the files are
    checked one at a time and the one it fails on is left out, so a bad
    contribution can't stop a release (or a deploy)."""
    thumbs.mkdir(parents=True, exist_ok=True)
    good, bad = [], []

    def run(batch: list[Path]) -> bool:
        r = subprocess.run([str(tool), "check", "--json", *(["--trusted"] if trusted else []), "--thumbs", str(thumbs),
                            *map(str, batch)], capture_output=True, text=True)
        if r.returncode not in (0, 1):
            return False
        for line in r.stdout.splitlines():
            e = json.loads(line)
            (bad if "problems" in e else good).append(e)
        return True

    for k in range(0, len(files), 200):
        batch = files[k:k + 200]
        if not run(batch):
            for f in batch:
                if not run([f]):
                    bad.append({"file": f.name, "problems": ["the checker failed on it"]})
    for p in sorted(thumbs.glob("*.png")):
        p.write_bytes(recompress_png(p.read_bytes()))
    return good, bad


def build(formulas: Path, examples: list[dict], contrib: Path | None, tool: Path) -> str:
    """Writes the thumbnails under `formulas` (formulas/examples/ and
    formulas/thumbs/, emptied first) and returns the page's body: the
    contributions in `contrib` (checked again here, so a file that should
    not have been merged is left out, not published) and the examples (the
    web app's formulas.json), with thumbnails drawn by the checker."""
    for d in ("examples", "thumbs"):
        shutil.rmtree(formulas / d, ignore_errors=True)
    with tempfile.TemporaryDirectory() as tmp:
        files = []
        for x in examples:
            vals = " ".join(f"{k}={x[k]:.2f}".rstrip("0").rstrip(".") for k in "abc" if x.get(k))
            head = [f"-- name: {x['name']}", f"-- description: {x['desc']}", "-- author: OpenDMD", "-- licence: CC0-1.0"]
            if vals:
                head.append(f"-- values: {vals}")
            p = Path(tmp) / f"{slugify(x['name'])}.psi"
            p.write_text("\n".join(head) + "\n" + x["src"].rstrip("\n") + "\n", encoding="utf-8")
            files.append(p)
        seeds, bad = check_gallery(tool, files, formulas / "examples", trusted=True)
        if bad or len(seeds) != len(examples):
            raise SystemExit(f"an example fails the gallery's checks: {bad}")
    entries: list[dict] = []
    if contrib is not None:
        if not contrib.is_dir():
            raise SystemExit(f"--gallery {contrib}: not a folder")
        # Regular files only: a link is never followed (lstat, not stat).
        listing = sorted(contrib.iterdir())
        files = [p for p in listing if p.suffix == ".psi" and not p.is_symlink() and p.is_file()][:MAX]
        for p in listing:
            if p.suffix == ".psi" and (p.is_symlink() or not p.is_file()):
                say(f"gallery: left out {p.name!r}: not a plain file (a link or a folder)")
        entries, bad = check_gallery(tool, files, formulas / "thumbs")
        for b in bad:
            say(f"gallery: left out {b['file']!r}: {'; '.join(b['problems'])}")
        known = {e["src"].strip() for e in seeds}
        dup = [e for e in entries if e["src"].strip() in known]
        for e in dup:
            say(f"gallery: left out {e['slug']!r}: the same formula as an example")
        entries = sorted((e for e in entries if e not in dup), key=lambda e: (e["name"].casefold(), e["slug"]))

    def tile(e: dict, pic: str) -> str:
        t = html.escape
        lic, url = LICENCE_LINKS[e["licence"]]
        name = e["name"]
        return (f'<li class="ftile"><a class="fpic" href="../f/{t(share_hash(e))}" tabindex="-1" aria-hidden="true">'
                f'<picture class="led"><img src="{t(pic)}" alt="" width="128" height="32" loading="lazy" decoding="async"></picture></a>'
                f'<h3>{t(name)}</h3><p class="fdesc">{t(e["description"])}</p>'
                f'<p class="fby">by {t(e["author"])} · <a href="{url}" rel="license">{lic}</a></p>'
                f'<p class="facts"><a class="cta" href="../f/{t(share_hash(e))}">Open</a>'
                f'<a class="cta alt" href="../demo/{t(share_hash(e, (name + " remix")[:60]))}">Remix</a></p></li>')
    community = "".join(tile(e, f"thumbs/{e['slug']}.png") for e in entries)
    ex = "".join(tile(e, f"examples/{e['slug']}.png") for e in seeds)
    how = (f'<a href="{REPO}/tree/main/gallery">how to add yours</a>')
    return (f'<h1>Formula gallery</h1>\n<p class="lead">Formulas are a few lines of maths that draw on the display, every pixel '
            f'at once. Open one to watch it, change its sliders and send it to your display; Remix opens it in the demo\'s '
            f'editor. Made one you like? Its page has <strong>Submit to the gallery</strong>, or read {how}.</p>\n'
            f'<h2 id="community">From the community</h2>\n'
            + (f'<ul class="ftiles">{community}</ul>\n' if community else
               '<p class="tiles-note">Nothing here yet: be the first. Make a formula in the <a href="../demo/#formulas">demo</a>, '
               'then Copy share link, and on its page, Submit to the gallery.</p>\n')
            + f'<h2 id="examples">Examples</h2>\n<p class="tiles-note">By OpenDMD, the ones the web app comes with (CC0).</p>\n'
            f'<ul class="ftiles">{ex}</ul>\n')


def marked(body: str) -> str:
    """The body as the site build writes it into the page: between the markers."""
    return f"{BEGIN}\n{body}{END}"


def splice(page: str, body: str) -> str:
    """The page with what is between its markers replaced by `body`."""
    if page.count(BEGIN) != 1 or page.count(END) != 1 or page.index(BEGIN) > page.index(END):
        raise SystemExit(f"{PAGE}: not one pair of gallery markers (a page from an older site build?)")
    head, rest = page.split(BEGIN)
    _, tail = rest.split(END)
    return head + marked(body) + tail


def main() -> int:
    ap = argparse.ArgumentParser(description="Rewrite a built site's formula gallery (formulas/) in place.")
    ap.add_argument("site", type=Path, help="the built site (its demo/formulas.json and formulas/index.html)")
    ap.add_argument("--tool", type=Path, required=True, help="opendmd-gallery, built")
    ap.add_argument("--gallery", type=Path, help="the contributions (the public site repo's gallery/)")
    a = ap.parse_args()
    page = a.site / PAGE
    if page.is_symlink() or not page.is_file():
        raise SystemExit(f"no {page}")
    examples = json.loads((a.site / "demo/formulas.json").read_text(encoding="utf-8"))
    text = page.read_text(encoding="utf-8")
    splice(text, "")  # the markers are there, before anything is rewritten
    body = build(page.parent, examples, a.gallery, a.tool)
    page.write_text(splice(text, body), encoding="utf-8")
    n, ex = body.count('src="thumbs/'), body.count('src="examples/')
    print(f"{PAGE}: {n} contribution{'s' * (n != 1)}, {ex} examples")
    return 0


if __name__ == "__main__":
    sys.exit(main())
