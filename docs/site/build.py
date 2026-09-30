#!/usr/bin/env python3
"""Pre-render the landing page into one static page per language.

The page is authored once, in English, in index.html; every translatable
element carries data-i18n="key" and the strings live in assets/js/i18n.js.
Translating in the browser alone leaves search engines with a single English
URL, so this script writes real pages instead:

    python3 build.py OUT_DIR   write the site to OUT_DIR:
                                 /            English
                                 /it/ /fr/ /es/ /de/
                               each with its own <title>, description,
                               canonical, hreflang, JSON-LD and sitemap entry
    python3 build.py --sync    refill the English text inside index.html
                               from i18n.js, so the source never drifts

Standard library only: it runs as-is on the GitHub Pages runner.
"""

import datetime
import html
import json
import pathlib
import re
import shutil
import sys

SITE = pathlib.Path(__file__).resolve().parent
BASE_URL = "https://polpo21.github.io/zero_layer/"
LANGS = ["en", "it", "fr", "es", "de"]
LOCALES = {"en": "en_US", "it": "it_IT", "fr": "fr_FR", "es": "es_ES", "de": "de_DE"}
VERSION_RE = re.compile(r"<strong>v(\d+\.\d+\.\d+)</strong>")


def page_url(lang):
    return BASE_URL if lang == "en" else f"{BASE_URL}{lang}/"


def load_i18n():
    """Parse the `key: "value"` pairs of each language block in i18n.js."""
    src = (SITE / "assets/js/i18n.js").read_text(encoding="utf-8")
    starts = sorted((src.index(f"    {lang}: {{"), lang) for lang in LANGS)
    dicts = {}
    for n, (start, lang) in enumerate(starts):
        end = starts[n + 1][0] if n + 1 < len(starts) else len(src)
        pairs = re.findall(r'(\w+):\s*("(?:[^"\\]|\\.)*")', src[start:end])
        dicts[lang] = {k: json.loads(v) for k, v in pairs}
    en_keys = set(dicts["en"])
    for lang in LANGS[1:]:
        missing = en_keys - set(dicts[lang])
        if missing:
            print(f"warning: {lang} falls back to English for {sorted(missing)}")
    return dicts


def is_markup_key(key):
    """Mirror of translate() in main.js: these values carry inline tags."""
    return bool(re.search(r"_(p1|p2|html)$", key) or re.fullmatch(r"a\d+", key)) or key == "rel_notice"


def find_element(doc, attr_pos):
    """Return (start_tag_end, close_start) for the element owning attr_pos."""
    tag_start = doc.rfind("<", 0, attr_pos)
    name = re.match(r"<([a-zA-Z0-9]+)", doc[tag_start:]).group(1)
    start_end = doc.index(">", attr_pos) + 1
    depth, pos = 1, start_end
    open_re = re.compile(rf"<{name}\b|</{name}\s*>")
    while depth:
        m = open_re.search(doc, pos)
        if not m:
            raise ValueError(f"unclosed <{name}> near offset {attr_pos}")
        depth += -1 if m.group(0).startswith("</") else 1
        pos = m.end()
        if depth == 0:
            return start_end, m.start()
    raise AssertionError


def apply_text(doc, strings, fallback):
    """Fill every data-i18n element and data-i18n-attr attribute."""
    for m in reversed(list(re.finditer(r'data-i18n="([^"]+)"', doc))):
        key = m.group(1)
        value = strings.get(key, fallback.get(key))
        if value is None:
            continue
        inner_start, inner_end = find_element(doc, m.start())
        text = value if is_markup_key(key) else html.escape(value, quote=False)
        doc = doc[:inner_start] + text + doc[inner_end:]

    def set_attrs(m):
        tag = m.group(0)
        spec = re.search(r'data-i18n-attr="([^"]+)"', tag).group(1)
        for pair in spec.split(","):
            attr, key = (p.strip() for p in pair.split(":"))
            value = strings.get(key, fallback.get(key))
            if value is None:
                continue
            value = html.escape(value)
            if re.search(rf'\s{attr}="', tag):
                tag = re.sub(rf'(\s{attr}=")[^"]*"', lambda a: a.group(1) + value + '"', tag)
            else:
                tag = tag[:-1] + f' {attr}="{value}">'
        return tag

    return re.sub(r"<[a-zA-Z][^<>]*data-i18n-attr=\"[^\"]+\"[^<>]*>", set_attrs, doc)


def set_meta(doc, selector_attr, name, value):
    pattern = rf'(<meta {selector_attr}="{re.escape(name)}" content=")[^"]*(")'
    new, n = re.subn(pattern, lambda m: m.group(1) + html.escape(value) + m.group(2), doc)
    if n != 1:
        raise ValueError(f"expected one <meta {selector_attr}={name}>")
    return new


def json_ld(lang, strings, version):
    faq = []
    for n in range(1, 20):
        q, a = strings.get(f"q{n}"), strings.get(f"a{n}")
        if not q or not a:
            break
        answer = html.unescape(re.sub(r"<[^>]+>", "", a))
        faq.append({"@type": "Question", "name": q,
                    "acceptedAnswer": {"@type": "Answer", "text": answer}})
    graph = {
        "@context": "https://schema.org",
        "@graph": [
            {
                "@type": "SoftwareApplication",
                "@id": BASE_URL + "#software",
                "name": "Zero Layer",
                "alternateName": "zl",
                "description": strings["meta_desc"],
                "applicationCategory": "DeveloperApplication",
                "operatingSystem": "Linux",
                "url": BASE_URL,
                "downloadUrl": "https://github.com/polpo21/zero_layer/releases/latest",
                "softwareVersion": version,
                "programmingLanguage": "Rust",
                "license": "https://opensource.org/licenses/MIT",
                "codeRepository": "https://github.com/polpo21/zero_layer",
                "image": BASE_URL + "og-image.png",
                "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
                "author": {"@type": "Person", "name": "polpo21", "url": "https://github.com/polpo21"},
            },
            {
                "@type": "WebPage",
                "@id": page_url(lang),
                "url": page_url(lang),
                "name": strings["meta_title"],
                "description": strings["meta_desc"],
                "inLanguage": lang,
                "about": {"@id": BASE_URL + "#software"},
            },
            {"@type": "FAQPage", "inLanguage": lang, "mainEntity": faq},
        ],
    }
    return json.dumps(graph, ensure_ascii=False, indent=2)


def render(template, lang, dicts):
    strings, fallback = dicts[lang], dicts["en"]
    merged = {**fallback, **strings}
    doc = apply_text(template, merged, fallback)
    version = VERSION_RE.search(merged["rel_notice"]).group(1)

    doc = re.sub(r'<html lang="[^"]*"', f'<html lang="{lang}"', doc, count=1)
    doc = re.sub(r"<title>[^<]*</title>", f"<title>{html.escape(merged['meta_title'])}</title>", doc, count=1)
    doc = set_meta(doc, "name", "description", merged["meta_desc"])
    doc = set_meta(doc, "property", "og:title", merged["meta_title"])
    doc = set_meta(doc, "property", "og:description", merged["meta_desc"])
    doc = set_meta(doc, "property", "og:url", page_url(lang))
    doc = set_meta(doc, "property", "og:locale", LOCALES[lang])
    doc = set_meta(doc, "name", "twitter:title", merged["meta_title"])
    doc = set_meta(doc, "name", "twitter:description", merged["meta_desc"])
    doc = re.sub(r'<link rel="canonical" href="[^"]*">', f'<link rel="canonical" href="{page_url(lang)}">', doc, count=1)
    doc = re.sub(
        r'(<script type="application/ld\+json">)\s*.*?\s*(</script>)',
        lambda m: m.group(1) + "\n" + json_ld(lang, merged, version) + "\n" + m.group(2),
        doc, count=1, flags=re.S,
    )

    root = "" if lang == "en" else "../"
    marker = f'<meta name="zl-page-lang" content="{lang}">\n<meta name="zl-root" content="{root}">\n'
    doc = doc.replace('<meta charset="UTF-8">\n', '<meta charset="UTF-8">\n' + marker, 1)
    if root:
        doc = re.sub(r'((?:href|src)=")(assets/|favicon\.svg)', rf"\g<1>{root}\g<2>", doc)
    return doc, version


def sitemap(today):
    alternates = "".join(
        f'\n    <xhtml:link rel="alternate" hreflang="{lang}" href="{page_url(lang)}"/>' for lang in LANGS
    ) + f'\n    <xhtml:link rel="alternate" hreflang="x-default" href="{BASE_URL}"/>'
    urls = "".join(
        f"\n  <url>\n    <loc>{page_url(lang)}</loc>\n    <lastmod>{today}</lastmod>{alternates}\n  </url>"
        for lang in LANGS
    )
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"\n'
        '        xmlns:xhtml="http://www.w3.org/1999/xhtml">'
        f"{urls}\n</urlset>\n"
    )


def build(out):
    dicts = load_i18n()
    template = (SITE / "index.html").read_text(encoding="utf-8")
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    shutil.copytree(SITE / "assets", out / "assets", ignore=shutil.ignore_patterns("og-image.src.html"))
    for name in ["og-image.png", "favicon.svg", "robots.txt"]:
        shutil.copy2(SITE / name, out / name)
    for lang in LANGS:
        doc, version = render(template, lang, dicts)
        target = out / "index.html" if lang == "en" else out / lang / "index.html"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(doc, encoding="utf-8")
        print(f"{lang}: {target.relative_to(out)} (v{version})")
    (out / "sitemap.xml").write_text(sitemap(datetime.date.today().isoformat()), encoding="utf-8")


def sync():
    dicts = load_i18n()
    path = SITE / "index.html"
    doc = path.read_text(encoding="utf-8")
    path.write_text(apply_text(doc, dicts["en"], dicts["en"]), encoding="utf-8")
    print("index.html: English text refreshed from i18n.js")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    if sys.argv[1] == "--sync":
        sync()
    else:
        build(pathlib.Path(sys.argv[1]).resolve())
