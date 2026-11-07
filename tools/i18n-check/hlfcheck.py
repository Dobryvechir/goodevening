"""Compare translated HLF help files with assets/help/GmEng.hlf.

Usage: python tools/i18n-check/hlfcheck.py [options] assets/help/GmXxx.hlf [...]
Options (before the files):
  --untranslated          also warn about lines identical to English
  --topics A,B            check only these topics
  --range First..Last     check only the topics from First to Last (English order)
  --eng path.hlf          English reference (default assets/help/GmEng.hlf)

Errors: topic order, aliases, link targets per topic (web links may differ),
key names in key-table rows (' #Key#  text'), missing $ header.
Warnings: big line-count or paragraph-count differences, lines identical to
English (with --untranslated).
"""
import re, sys, os
from collections import OrderedDict

HERE = os.path.dirname(os.path.abspath(__file__))
ENG = os.path.join(HERE, '..', '..', 'assets', 'help', 'GmEng.hlf')
LINK = re.compile(r'~[^~]*~@([^@]+)@')
KEY = re.compile(r'(?:(?:Left|Right|R)?(?:Ctrl|Alt|Shift|RCtrl|RAlt)\+)*(?:F\d+|[A-Z0-9]|Ins|Del|Enter|Esc|Tab|Home|End|PgUp|PgDn|Space|BS|Gray [+*/-]|Up|Down|Left|Right)(?:, .*)?')


def parse(path):
    topics = OrderedDict()
    aliases = []
    cur = None
    for raw in open(path, encoding='utf-8-sig').read().split('\n'):
        raw = raw.rstrip('\r')
        if raw.startswith('@') and not raw.startswith('@@') and len(raw) > 1 and raw not in ('@-', '@+'):
            rest = raw[1:]
            if '=' in rest:
                aliases.append(rest)
                continue
            cur = rest.strip()
            topics[cur] = []
            continue
        if cur is not None:
            topics[cur].append(raw)
    for k in topics:
        while topics[k] and not topics[k][-1].strip():
            topics[k].pop()
    return topics, aliases


def links(lines):
    return sorted(m for l in lines for m in LINK.findall(l) if not m.startswith(('http', 'mailto')))


def keys(lines):
    out = []
    for l in lines:
        m = re.match(r' #([^#]+)#', l)
        if m and KEY.fullmatch(m.group(1)):
            out.append(m.group(1))
    return sorted(out)


def paras(lines):
    return sum(1 for l in lines if re.match(r' \S', l) or l.startswith(' #'))


def main():
    args = sys.argv[1:]
    untr = '--untranslated' in args
    args = [a for a in args if a != '--untranslated']
    only = None
    eng = ENG
    rng = None
    while args and args[0].startswith('--'):
        if args[0] == '--topics':
            only = set(args[1].split(','))
        elif args[0] == '--eng':
            eng = args[1]
        elif args[0] == '--range':
            rng = args[1].split('..')
        else:
            sys.exit(f'unknown option {args[0]}')
        args = args[2:]
    et, ea = parse(eng)
    if rng:
        names = list(et)
        only = set(names[names.index(rng[0]):names.index(rng[1]) + 1])
    eng_lines = set(l for v in et.values() for l in v)
    bad = False
    for path in args:
        t, a = parse(path)
        errs, warns = [], []
        if only is None:
            if list(t) != list(et):
                miss = [k for k in et if k not in t]
                extra = [k for k in t if k not in et]
                errs.append(f'topic order differs; missing {miss[:20]} extra {extra[:20]}')
            if sorted(a) != sorted(ea):
                errs.append(f'aliases differ: {sorted(set(a) ^ set(ea))[:20]}')
        for k, el in et.items():
            if only is not None and k not in only:
                continue
            if k not in t:
                if only is not None:
                    errs.append(f'[{k}] missing')
                continue
            ol = t[k]
            if not any(l.startswith('$') for l in ol) and any(l.startswith('$') for l in el):
                errs.append(f'[{k}] no $ header line')
            if links(el) != links(ol):
                le, lo = links(el), links(ol)
                errs.append(f'[{k}] links: English-only {sorted(set(le) - set(lo)) or "(count)"} extra {sorted(set(lo) - set(le))} ({len(le)} vs {len(lo)})')
            if keys(el) != keys(ol):
                ke, ko = keys(el), keys(ol)
                errs.append(f'[{k}] key rows: English-only {[x for x in ke if x not in ko]} extra {[x for x in ko if x not in ke]} ({len(ke)} vs {len(ko)})')
            if abs(len(el) - len(ol)) > max(4, len(el) // 4):
                warns.append(f'[{k}] line count {len(el)} vs {len(ol)}')
            pe, po = paras(el), paras(ol)
            if abs(pe - po) > max(2, pe // 5):
                warns.append(f'[{k}] paragraph/row count {pe} vs {po}')
            if untr and k != 'License':
                n = 0
                for l in ol:
                    txt = re.sub(r'#[^#]*#|~[^~]*~@[^@]*@|@\S+', '', l)
                    if l in eng_lines and len(re.findall(r'[A-Za-z]{3,}', txt)) >= 2 and not l.startswith('$ #'):
                        n += 1
                if n:
                    warns.append(f'[{k}] {n} line(s) identical to English')
        print(f'== {path}: {len(errs)} errors, {len(warns)} warnings')
        for e in errs:
            print('  ERROR', e)
        for w in warns:
            print('  warn ', w)
        bad |= bool(errs)
    sys.exit(1 if bad else 0)


main()
