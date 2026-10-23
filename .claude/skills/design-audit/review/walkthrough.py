#!/usr/bin/env python3
"""Builds the design-review walkthrough page: every styleguide example with its
committed snapshots, and how each component follows Krug's principles.

  python3 .claude/skills/design-audit/review/walkthrough.py OUT.html

Reads crates/treq-ui/styleguide/manifest.json (snapshots, code), the
`.describe(..)` rules in crates/treq-ui/examples/styleguide.rs, and krug.json
beside this script (the per-component notes, which the reviewer writes).
"""
import base64, html, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '../../../..'))
SG = os.path.join(ROOT, 'crates/treq-ui/styleguide/')
OUT = sys.argv[1]

manifest = json.load(open(SG + 'manifest.json'))
examples = {e['id']: e for e in manifest['examples']}
review = json.load(open(os.path.join(HERE, 'krug.json')))
P = review['principles']
src = open(os.path.join(ROOT, 'crates/treq-ui/examples/styleguide.rs')).read()
rules = {
    k: re.sub(r'\s*\\\n\s*', ' ', v).replace('\\"', '"')
    for k, v in re.findall(r'\.describe\(\s*"(\w+)",\s*"((?:[^"\\]|\\.)*)"', src)
}


def data_uri(path):
    return 'data:image/png;base64,' + base64.b64encode(open(SG + path, 'rb').read()).decode()


def variant_label(v):
    if v.get('is_default'):
        return 'default'
    return v['file'].rsplit('/', 1)[1][:-4].replace(',', ', ')


def shots(eid):
    e, out = examples[eid], []
    for theme in ('light', 'dark'):
        seen, figs = set(), []
        for v in (v for v in e['variants'] if v['theme'] == theme):
            if v['hash'] in seen and not v.get('is_default'):
                continue
            seen.add(v['hash'])
            alt = html.escape(f"{e['component']} {variant_label(v)}, {theme}")
            figs.append(
                f'<figure><img src="{data_uri(v["file"])}" width="{v["width"]}" alt="{alt}">'
                f'<figcaption>{html.escape(variant_label(v))}</figcaption></figure>'
            )
        out.append(f'<div class="shots t-{theme}">{"".join(figs)}</div>')
    return ''.join(out)


def code(eid):
    e, s = examples[eid], ''
    for part in e['code']:
        if 'text' in part:
            s += part['text']
            continue
        d = next(k for k in e['knobs'] if k['name'] == part['knob'])['default']
        s += json.dumps(d) if isinstance(d, str) else str(d).lower() if isinstance(d, bool) else str(d)
    return f'<details><summary>Code</summary><pre><code>{html.escape(s)}</code></pre></details>'


def chip(k):
    return f'<span class="chip c-{k}">{P[k]}</span>'


sections, matrix = [], ''
for c in review['components']:
    keys = [n['principle'] for n in c['notes']]
    used = [k for k in P if k in keys]
    exs = ''.join(
        f'<div class="ex"><h4>{html.escape(examples[i]["title"])} <span class="eid">{i}</span></h4>'
        f'{shots(i)}{code(i)}</div>'
        for i in c['examples']
    )
    rows = ''.join(
        f'<tr><th>{chip(n["principle"])}</th><td>{html.escape(n["how"]).replace("`", "")}</td></tr>'
        for n in c['notes']
    )
    compound = f'<p class="compound"><b>Compound.</b> {html.escape(c["compound"])}</p>' if c.get('compound') else ''
    gap = f'<p class="gap"><b>Known gap.</b> {html.escape(c["gap"])}</p>' if c.get('gap') else ''
    sections.append(
        f'<section id="{c["name"].lower()}"><header><h2>{c["name"]}</h2>'
        f'<div class="chips">{"".join(chip(k) for k in used)}</div></header>'
        f'<p class="rule">{html.escape(rules.get(c["name"], ""))}</p>{compound}'
        f'<div class="body"><div class="exs">{exs}</div><table class="krug">{rows}</table></div>{gap}</section>'
    )
    matrix += f'<tr><th><a href="#{c["name"].lower()}">{c["name"]}</a></th>' + ''.join(
        f'<td>{"●" if k in keys else ""}</td>' for k in P
    ) + '</tr>'

matrix = (
    '<table class="matrix"><thead><tr><th></th>'
    + ''.join(f'<th>{v}</th>' for v in P.values())
    + f'</tr></thead><tbody>{matrix}</tbody></table>'
)
nav = ''.join(f'<a href="#{c["name"].lower()}">{c["name"]}</a>' for c in review['components'])
page = open(os.path.join(HERE, 'walkthrough.html')).read()
open(OUT, 'w').write(
    page.replace('%%NAV%%', nav).replace('%%MATRIX%%', matrix).replace('%%SECTIONS%%', '\n'.join(sections))
)
print(f'wrote {OUT}')
