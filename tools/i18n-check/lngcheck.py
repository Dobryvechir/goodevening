"""Validate a .lng file against assets/lang/GmEng.lng.

Usage: python tools/i18n-check/lngcheck.py assets/lang/GmXxx.lng [more.lng ...]

Checks: header, identical //[Id] sequence, one string per id, placeholders
({0}, {1:02}, %VAR%, \\n, \\t) preserved, hotkey marker '&' present exactly
once when English has one (and absent when English has none), empty strings
match (except MCMLItems* plural suffixes, which are language-specific), no unescaped quotes, leading/trailing spaces preserved.
Warnings: strings identical to English (possibly untranslated).
Exit code 1 when an error is found.
"""
import re, sys, os

HERE = os.path.dirname(os.path.abspath(__file__))
ENG = os.path.join(HERE, '..', '..', 'assets', 'lang', 'GmEng.lng')
PH = re.compile(r'\{\d+(?::[^}]*)?\}|%[A-Za-z_]+%|\\[nt]')


def load(path):
    lines = open(path, encoding='utf-8').read().split('\n')
    if lines and lines[-1] == '':
        lines.pop()
    errs = []
    if not lines or not lines[0].startswith('.Language='):
        errs.append('line 1: missing .Language= header')
    pairs = []
    pend = None
    for n, l in enumerate(lines, 1):
        if l.endswith('\r'):
            errs.append(f'line {n}: CRLF line ending (use LF)')
            l = l[:-1]
        m = re.fullmatch(r'//\[(\w+)\]', l)
        if m:
            if pend is not None:
                errs.append(f'line {n}: id {pend[0]} has no string')
            pend = (m.group(1), n)
            continue
        if l.startswith('"'):
            if pend is None:
                errs.append(f'line {n}: string without id')
                continue
            if not (len(l) >= 2 and l.endswith('"')):
                errs.append(f'line {n}: string must end with a quote')
            inner = l[1:-1]
            if re.search(r'(?<!\\)(?:\\\\)*"', inner):
                errs.append(f'line {n}: unescaped quote inside string (use \\")')
            pairs.append((pend[0], inner, n))
            pend = None
    return pairs, errs


def hot(s):
    return len(re.findall(r'(?<!&)&(?!&)', s.replace('&&', '')))


def check(path, eng):
    pairs, errs = load(path)
    warns = []
    if [p[0] for p in pairs] != [p[0] for p in eng]:
        ids = [p[0] for p in pairs]
        eids = [p[0] for p in eng]
        for i, (a, b) in enumerate(zip(ids, eids)):
            if a != b:
                errs.append(f'id sequence differs at #{i + 1}: {a} (line {pairs[i][2]}) vs English {b}')
                break
        if len(ids) != len(eids):
            errs.append(f'{len(ids)} strings vs {len(eids)} in English')
        return errs, warns
    for (id_, s, n), (_, e, _) in zip(pairs, eng):
        if (e == '') != (s == '') and not id_.startswith('MCMLItems'):
            errs.append(f'line {n} [{id_}]: empty-ness differs from English')
            continue
        if sorted(PH.findall(s)) != sorted(PH.findall(e)):
            errs.append(f'line {n} [{id_}]: placeholders {sorted(PH.findall(s))} vs English {sorted(PH.findall(e))}')
        he, hs = hot(e), hot(s)
        if he and hs != 1:
            errs.append(f'line {n} [{id_}]: needs exactly one & hotkey (English: {e!r})')
        if not he and hs:
            errs.append(f'line {n} [{id_}]: English has no & hotkey, translation has one')
        le = len(e) - len(e.lstrip(' '))
        ls = len(s) - len(s.lstrip(' '))
        if (le > 0) != (ls > 0):
            warns.append(f'line {n} [{id_}]: leading spaces differ ({le} vs {ls})')
        te = len(e) - len(e.rstrip(' '))
        ts = len(s) - len(s.rstrip(' '))
        if (te > 0) != (ts > 0):
            warns.append(f'line {n} [{id_}]: trailing spaces differ ({te} vs {ts})')
        if s.replace('&', '') == e.replace('&', '') and len(re.findall(r'[A-Za-z]{3,}', e)) >= 2:
            warns.append(f'line {n} [{id_}]: identical to English: {e!r}')
    return errs, warns


def main():
    eng, eerrs = load(ENG)
    assert not eerrs, eerrs
    bad = False
    for path in sys.argv[1:]:
        errs, warns = check(path, eng)
        print(f'== {path}: {len(errs)} errors, {len(warns)} warnings')
        for e in errs[:200]:
            print('  ERROR', e)
        for w in warns[:200]:
            print('  warn ', w)
        bad |= bool(errs)
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
