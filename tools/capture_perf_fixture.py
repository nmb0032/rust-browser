#!/usr/bin/env python3
"""Capture a live page as an offline fixture for `benches/frame.rs`.

    python3 tools/capture_perf_fixture.py https://en.wikipedia.org/wiki/Rust_(programming_language) wikipedia-rust

Writes tests/fixtures/perf/<name>/index.html plus css/*.css. Linked
stylesheets (and their @imports) are downloaded and rewritten to local files;
scripts and inline event handlers are removed because headless loads reject
them. Other subresources keep their remote URLs and fail as diagnostics under
the local resource policy. Captures are third-party content and are
gitignored: they are for local measurement only.
"""

import html
import pathlib
import re
import sys
import urllib.parse
import urllib.request
from html.parser import HTMLParser

ROOT = pathlib.Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "perf"
USER_AGENT = "Mozilla/5.0 (rust-browser perf fixture capture)"
IMPORT = re.compile(r"""@import\s+(?:url\(\s*)?["']?([^"')\s;]+)["']?\s*\)?([^;]*);""", re.I)


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        charset = response.headers.get_content_charset() or "utf-8"
        return response.geturl(), response.read().decode(charset, "replace")


class Capture:
    def __init__(self, out):
        self.out = out
        self.sheets = {}
        (out / "css").mkdir(parents=True, exist_ok=True)

    def sheet(self, url, depth=0):
        """Download a stylesheet and return its local relative path."""
        url = urllib.parse.urldefrag(url)[0]
        if url in self.sheets:
            return self.sheets[url]
        name = f"css/{len(self.sheets)}.css"
        self.sheets[url] = name
        try:
            final, text = fetch(url)
        except Exception as error:  # noqa: BLE001 - keep capturing other sheets
            print(f"  skip {url}: {error}", file=sys.stderr)
            text, final = "", url

        def local_import(match):
            if depth >= 4:
                return ""
            target = urllib.parse.urljoin(final, match.group(1))
            # Sibling of this sheet inside css/.
            return f'@import "{self.sheet(target, depth + 1)[4:]}"{match.group(2)};'

        text = IMPORT.sub(local_import, text)
        (self.out / name).write_text(text, encoding="utf-8")
        print(f"  {name} <- {url} ({len(text)} bytes)")
        return name


class Rewriter(HTMLParser):
    def __init__(self, base, capture):
        super().__init__(convert_charrefs=False)
        self.base = base
        self.capture = capture
        self.parts = []
        self.skip = 0

    def emit_tag(self, tag, attrs, closed):
        rendered = "".join(
            f" {name}" if value is None else f' {name}="{html.escape(value, quote=True)}"'
            for name, value in attrs
        )
        self.parts.append(f"<{tag}{rendered}{' /' if closed else ''}>")

    def start(self, tag, attrs, closed):
        if tag == "script":
            if not closed:
                self.skip += 1
            return
        if self.skip:
            return
        if tag == "base":
            return
        attrs = [(n, v) for n, v in attrs if not n.lower().startswith("on")]
        if tag == "link":
            values = dict(attrs)
            rel = (values.get("rel") or "").lower().split()
            href = values.get("href")
            if "stylesheet" not in rel or not href or "alternate" in rel:
                return
            local = self.capture.sheet(urllib.parse.urljoin(self.base, html.unescape(href)))
            attrs = [(n, local if n == "href" else v) for n, v in attrs]
        elif tag in ("a", "area"):
            # Absolute links keep fragment/same-document behavior irrelevant to layout.
            attrs = [
                (n, urllib.parse.urljoin(self.base, html.unescape(v)) if n == "href" and v else v)
                for n, v in attrs
            ]
        self.emit_tag(tag, attrs, closed)

    def handle_starttag(self, tag, attrs):
        self.start(tag, attrs, False)

    def handle_startendtag(self, tag, attrs):
        self.start(tag, attrs, True)

    def handle_endtag(self, tag):
        if tag == "script":
            self.skip = max(0, self.skip - 1)
            return
        if not self.skip:
            self.parts.append(f"</{tag}>")

    def passthrough(self, text):
        if not self.skip:
            self.parts.append(text)

    def handle_data(self, data):
        self.passthrough(data)

    def handle_entityref(self, name):
        self.passthrough(f"&{name};")

    def handle_charref(self, name):
        self.passthrough(f"&#{name};")

    def handle_comment(self, data):
        self.passthrough(f"<!--{data}-->")

    def handle_decl(self, decl):
        self.passthrough(f"<!{decl}>")

    def handle_pi(self, data):
        self.passthrough(f"<?{data}>")

    def unknown_decl(self, data):
        self.passthrough(f"<![{data}]>")


def main():
    if len(sys.argv) != 3 or not re.fullmatch(r"[A-Za-z0-9_-]+", sys.argv[2]):
        sys.exit(__doc__)
    url, name = sys.argv[1], sys.argv[2]
    out = ROOT / name
    final, source = fetch(url)
    print(f"{final}: {len(source)} bytes")
    rewriter = Rewriter(final, Capture(out))
    rewriter.feed(source)
    rewriter.close()
    (out / "index.html").write_text("".join(rewriter.parts), encoding="utf-8")
    print(f"wrote {out / 'index.html'}")


if __name__ == "__main__":
    main()
